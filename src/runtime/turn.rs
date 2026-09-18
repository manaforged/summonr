use super::compaction::force_compact;
use super::*;
use crate::ProviderFailureKind;
use crate::durable::driver::{TurnWork, Verdict};
use crate::durable::id::{Attempt, Call};
use crate::durable::work::{Decision, Observation, Stage};
use crate::workflow;
use std::future::Future;
use std::time::Duration;

use crate::bus::{envelope, is_bus_tool};
use crate::policy::Grant;
use crate::prompt::harness_section;
use crate::provider::StopReason;
use crate::session::decorate;
use crate::tools::{ToolOutput, needed_grant, readonly};
use crate::{Receipt, WorkKind, WorkProgress, goal, jobs, plan, review, scout, verify};

const RETRY_LIMIT: usize = 6;
const RETRY_BASE_SECS: u64 = 2;
const RETRY_MAX_SECS: u64 = 60;
const RETRY_AFTER_MAX_SECS: u64 = 600;
const FALLBACK_RETRIES: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum GrantAnswer {
    Deny,
    Allow,
    Always,
}

fn parse_decision(text: &str, persist: bool) -> Option<GrantAnswer> {
    match text.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" | "a" | "allow" => Some(GrantAnswer::Allow),
        "always" if persist => Some(GrantAnswer::Always),
        "always" => Some(GrantAnswer::Allow),
        "n" | "no" | "d" | "deny" => Some(GrantAnswer::Deny),
        _ => None,
    }
}

pub(super) enum WaitError {
    Deadline(Duration),
    Provider(Error),
}

impl WaitError {
    pub(super) fn millis(&self, started: Instant) -> u64 {
        let elapsed = super::elapsed_millis(started);
        match self {
            Self::Deadline(remaining) => {
                elapsed.max(u64::try_from(remaining.as_millis()).unwrap_or(u64::MAX))
            }
            Self::Provider(_) => elapsed,
        }
    }

    pub(super) fn error(self) -> Error {
        match self {
            Self::Deadline(_) => Error::Config("work-time limit reached".to_string()),
            Self::Provider(error) => error,
        }
    }
}

impl<P: Provider> Loop<P> {
    fn take_mail(
        &mut self,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let mut injected = false;
        while let Some(input) = self.steer.pop_front() {
            self.session.steer(&input).map_err(super::session_err)?;
            messages.push(Message::user(input));
            injected = true;
        }
        if let Some(shared) = &self.steer_shared
            && let Ok(mut queue) = shared.lock()
        {
            while let Some(input) = queue.pop_front() {
                self.session.steer(&input).map_err(super::session_err)?;
                messages.push(Message::user(input));
                injected = true;
            }
        }
        let (mail, overflow) = self.take_bus_mail(5)?;
        for mail in mail {
            let input = UserInput::from(envelope(&mail));
            self.session.steer(&input).map_err(super::session_err)?;
            messages.push(Message::user(input));
            injected = true;
            self.push_event(
                events,
                emit,
                Event::Mail {
                    from: mail.from,
                    text: mail.text,
                },
            )?;
        }
        if overflow > 0 {
            let note = format!(
                "[summon bus: {overflow} more message(s) waiting — use the inbox tool to drain them]"
            );
            let input = UserInput::from(note);
            self.session.steer(&input).map_err(super::session_err)?;
            messages.push(Message::user(input));
            injected = true;
        }
        if injected {
            self.commit(messages);
        }
        Ok(())
    }

    async fn complete_retry(
        &mut self,
        messages: &mut Vec<Message>,
        tools: &[ToolDefinition],
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Option<(AssistantMessage, String, usize, Attempt, u64)>> {
        let role = self.round_role();
        let mut retries = 0_usize;
        let mut repairs = 0_usize;
        let mut overflows = 0_usize;
        let mut usage = Usage::default();
        loop {
            let request = self.completion_request(role, messages, tools);
            let prefix_hash = request.prefix_hash();
            let request_chars = request_chars(messages, tools);
            let durable = request.durable_bytes().map_err(|error| {
                Error::Config(format!("cannot encode provider request: {error}"))
            })?;
            let durable_attempt = self
                .session
                .start_provider(&request.model, super::exposure(&request, &durable))
                .map_err(super::session_err)?;
            let started = Instant::now();
            let remaining = self.remaining_time();
            let provider = self
                .seats
                .get(&role)
                .map_or(&self.provider, |(provider, _)| provider);
            let completion = provider.stream(&request, &mut self.session.partial);
            let reply = Self::wait(completion, remaining).await;
            match reply {
                Ok(message) => {
                    return Ok(Some((
                        message,
                        prefix_hash,
                        request_chars,
                        durable_attempt,
                        super::elapsed_millis(started),
                    )));
                }
                Err(error @ WaitError::Deadline(_)) => {
                    let millis = error.millis(started);
                    self.session
                        .fail_provider(durable_attempt, "work-time limit reached", millis, true)
                        .map_err(super::session_err)?;
                    self.sync_usage(emit);
                    return Ok(None);
                }
                Err(WaitError::Provider(error)) => {
                    self.session
                        .fail_provider(
                            durable_attempt,
                            &error.to_string(),
                            super::elapsed_millis(started),
                            super::billable(&error),
                        )
                        .map_err(super::session_err)?;
                    self.sync_usage(emit);
                    if self.round_abort().is_some() {
                        return Ok(None);
                    }
                    let text = error.to_string();
                    let failure = error.provider_failure();
                    if failure.is_some_and(|failure| {
                        !failure.partial_output
                            && failure.kind == ProviderFailureKind::MalformedToolCall
                    }) {
                        if repairs >= RETRY_LIMIT {
                            return Err(error);
                        }
                        repairs += 1;
                        for message in messages.iter_mut().rev() {
                            if message.role == crate::provider::Role::Assistant {
                                for call in &mut message.tool_calls {
                                    if serde_json::from_str::<serde_json::Value>(
                                        &call.function.arguments,
                                    )
                                    .is_err()
                                    {
                                        call.function.arguments = "{}".to_string();
                                    }
                                }
                                break;
                            }
                        }
                        let input = UserInput::from(
                            "[summon control: the provider rejected the previous tool call because \
                             its arguments were invalid JSON. Retry that action with one valid tool \
                             call. Split a large file write into smaller edits if needed.]",
                        );
                        self.session.steer(&input).map_err(super::session_err)?;
                        messages.push(Message::user(input));
                        self.push_event(
                            events,
                            emit,
                            Event::Note {
                                text: "malformed tool-call JSON — requesting a repaired call"
                                    .to_string(),
                            },
                        )?;
                        continue;
                    }
                    if failure
                        .is_some_and(|failure| failure.kind == ProviderFailureKind::ContextOverflow)
                    {
                        if overflows >= RETRY_LIMIT {
                            return Err(error);
                        }
                        overflows += 1;
                        if let Some(cap) = crate::provider::input_cap(&text) {
                            self.set_context_window(Some(cap));
                        }
                        self.auto_compact(messages, tools, &mut usage, events, emit)
                            .await?;
                        force_compact(messages);
                        self.push_event(
                            events,
                            emit,
                            Event::Note {
                                text: "context overflow — compacted old tool results, retrying"
                                    .to_string(),
                            },
                        )?;
                        continue;
                    }
                    let retryable = failure.is_some_and(|failure| {
                        (!failure.partial_output || failure.kind == ProviderFailureKind::Transport)
                            && matches!(
                                failure.kind,
                                ProviderFailureKind::RateLimited
                                    | ProviderFailureKind::Server
                                    | ProviderFailureKind::Transport
                            )
                    });
                    if self.boosted(role)
                        && failure.is_some_and(|failure| {
                            failure.kind == ProviderFailureKind::Http && failure.status == Some(400)
                        })
                    {
                        self.guard.reject();
                        self.push_event(
                            events,
                            emit,
                            Event::Note {
                                text: "the endpoint rejected the raised effort; continuing at the configured effort"
                                    .to_string(),
                            },
                        )?;
                        continue;
                    }
                    let limit = if self.fallbacks.is_empty() || self.seats.contains_key(&role) {
                        RETRY_LIMIT
                    } else {
                        FALLBACK_RETRIES
                    };
                    if !retryable || retries >= limit {
                        if self.seats.contains_key(&role) {
                            return Err(error);
                        }
                        let Some((provider, model)) = self.fallbacks.pop_front() else {
                            return Err(error);
                        };
                        let from = std::mem::replace(&mut self.model, model);
                        self.provider = provider;
                        self.context_sample = None;
                        retries = 0;
                        self.push_event(
                            events,
                            emit,
                            Event::Note {
                                text: format!(
                                    "{from} failed: {} — falling back to {}",
                                    text.lines().next().unwrap_or("unknown"),
                                    self.model
                                ),
                            },
                        )?;
                        continue;
                    }
                    let wait = failure
                        .and_then(|failure| failure.retry_after)
                        .map_or((RETRY_BASE_SECS << retries).min(RETRY_MAX_SECS), |after| {
                            after.clamp(1, RETRY_AFTER_MAX_SECS)
                        });
                    retries += 1;
                    self.push_event(
                        events,
                        emit,
                        Event::Note {
                            text: format!(
                                "provider error — retrying in {wait}s ({retries}/{limit}): {}",
                                text.lines().next().unwrap_or("unknown")
                            ),
                        },
                    )?;
                    tokio::time::sleep(Duration::from_secs(wait)).await;
                }
            }
        }
    }

    fn completion_request(
        &self,
        role: workflow::Role,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> CompletionRequest {
        CompletionRequest {
            session: self.session_id(),
            model: self.seat(role).1.to_string(),
            messages: messages.to_vec(),
            tools: tools.to_vec(),
            max_output_tokens: None,
            emit_deltas: true,
            effort: self.effort_for(role),
        }
    }

    pub(super) fn remaining_time(&self) -> Option<Duration> {
        let limit = self.budget.max_secs?.saturating_mul(1_000);
        Some(Duration::from_millis(
            limit.saturating_sub(self.session.replay().elapsed_millis),
        ))
    }

    pub(super) async fn wait<F>(
        completion: F,
        remaining: Option<Duration>,
    ) -> std::result::Result<AssistantMessage, WaitError>
    where
        F: Future<Output = Result<AssistantMessage>> + Send,
    {
        if let Some(remaining) = remaining {
            tokio::time::timeout(remaining, completion)
                .await
                .map_err(|_elapsed| WaitError::Deadline(remaining))?
                .map_err(WaitError::Provider)
        } else {
            completion.await.map_err(WaitError::Provider)
        }
    }

    pub(super) fn estimated_prompt_tokens(
        &self,
        messages: &[Message],
        tools: &[ToolDefinition],
    ) -> usize {
        let chars = request_chars(messages, tools);
        super::compaction::estimate(chars, self.context_sample)
    }

    pub async fn turn(&mut self, prompt: &str) -> Result<Turn> {
        self.turn_with(prompt, |_| {}).await
    }

    pub async fn turn_with(
        &mut self,
        input: impl Into<UserInput>,
        mut emit: impl FnMut(&Event),
    ) -> Result<Turn> {
        let _report = self.herdr.turn();
        let snapshot = crate::diff::snapshot(&self.workspace);
        let messages = self.begin_turn(input.into())?;
        let mut events = Vec::new();
        let mut usage = Usage::default();
        let mut rounds = 0;
        let remaining = self.remaining_time();
        let before = self.session.replay().elapsed_millis;
        let started = Instant::now();
        let turn = self.run_turn(
            messages,
            snapshot.clone(),
            &mut emit,
            &mut events,
            &mut usage,
            &mut rounds,
        );
        let result = match remaining {
            Some(remaining) => tokio::time::timeout(remaining, turn)
                .await
                .map_err(|_elapsed| ()),
            None => Ok(turn.await),
        };
        let accounted = self.session.replay().elapsed_millis.saturating_sub(before);
        let millis = super::elapsed_millis(started).saturating_sub(accounted);
        if millis > 0 {
            self.session
                .charge(
                    Meter {
                        millis,
                        ..Meter::default()
                    },
                    "unaccounted turn time",
                    &self.model,
                    false,
                )
                .map_err(super::session_err)?;
        }
        self.sync_usage(&mut emit);
        if let Ok(Ok(turn)) = &result {
            let payload = serde_json::json!({
                "event": "stop",
                "text": turn.text,
                "aborted": turn.aborted,
            });
            if let Some(note) = self.hook(self.hooks.stop.clone(), payload).await {
                emit(&Event::Note {
                    text: format!("stop hook: {note}"),
                });
            }
        }
        match result {
            Ok(result) => result,
            Err(()) => {
                self.abort_turn(
                    Some("work-time limit reached".to_string()),
                    &mut events,
                    &mut emit,
                    &mut usage,
                    rounds,
                    snapshot,
                )
                .await
            }
        }
    }

    pub async fn turn_mail_with(&mut self, emit: impl FnMut(&Event)) -> Result<Turn> {
        if self.mail.is_empty() {
            return Err(Error::Config("there is no pending peer mail".to_string()));
        }
        self.turn_with(
            "[summon control: respond to the pending peer mail in this Work]",
            emit,
        )
        .await
    }

    async fn run_turn(
        &mut self,
        mut messages: Vec<Message>,
        snapshot: String,
        mut emit: impl FnMut(&Event),
        events: &mut Vec<Event>,
        usage: &mut Usage,
        reached: &mut usize,
    ) -> Result<Turn> {
        let abort_reason;
        self.settle_mcp(events, &mut emit).await?;
        self.checkpoint(events, &mut emit)?;
        let tools = self.advertised_tools().to_vec();
        if std::mem::take(&mut self.accepted) {
            self.push_event(
                events,
                &mut emit,
                Event::Note {
                    text: "plan accepted — starting implementation".to_string(),
                },
            )?;
        }
        self.plan_first(&mut messages, usage, events, &mut emit)
            .await?;
        self.scout(&mut messages, usage, events, &mut emit).await?;

        loop {
            while self.budget.max_tokens.is_some() && self.child_budget_reserved() > 0 {
                self.await_children(events, &mut emit, usage).await?;
            }
            *reached = reached.saturating_add(1);
            let round = *reached;
            if let Some(reason) = self.guard.stuck() {
                let question = format!("{reason}. Let the turn continue?");
                if self.approve(question, false, events, &mut emit).await? == GrantAnswer::Deny {
                    abort_reason = Some(reason);
                    break;
                }
                self.guard.forgive();
            }
            if let Some(reason) = self.round_abort() {
                abort_reason = Some(reason);
                break;
            }
            let round_tools = tools.as_slice();
            if let Err(error) = self
                .prepare_round(&mut messages, round_tools, usage, events, &mut emit)
                .await
            {
                if let Some(reason) = self.round_abort() {
                    abort_reason = Some(reason);
                    break;
                }
                return Err(error);
            }
            if let Some(reason) = self.round_abort() {
                abort_reason = Some(reason);
                break;
            }
            let Some((assistant, prefix_hash, request_chars, durable_attempt, live_millis)) = self
                .complete_retry(&mut messages, round_tools, events, &mut emit)
                .await?
            else {
                abort_reason = self.round_abort();
                break;
            };
            if assistant.usage.prompt_tokens > 0 {
                self.context_sample = Some((request_chars, assistant.usage.prompt_tokens));
            }
            let role = self.round_role();
            self.record_usage(
                &assistant,
                prefix_hash,
                role,
                Some(live_millis),
                usage,
                events,
                &mut emit,
            )?;
            let stop_reason = assistant.stop_reason;
            let meter = super::meter(&assistant.usage, live_millis);
            let reasoning = assistant.reasoning_content.clone().unwrap_or_default();
            let message = assistant.into_message();
            let calls = message.tool_calls.clone();
            messages.push(message.clone());
            if stop_reason == StopReason::Length && calls.is_empty() {
                self.session
                    .finish_provider(durable_attempt, &message.content, &reasoning, meter)
                    .map_err(super::session_err)?;
                let control = if message.content.trim().is_empty() && !reasoning.is_empty() {
                    UserInput::from(
                        "/no_think [summon control: the previous round used its output on \
                         reasoning. Stop reasoning and give the concise final answer now.]",
                    )
                } else {
                    UserInput::from(
                        "[summon control: the previous round reached its output limit. Continue \
                         briefly from the partial answer.]",
                    )
                };
                self.session.steer(&control).map_err(super::session_err)?;
                messages.push(Message::user(control));
                self.push_event(
                    events,
                    &mut emit,
                    Event::Note {
                        text: "round output limit reached — continuing with the same context"
                            .to_string(),
                    },
                )?;
                self.commit(&messages);
                continue;
            }
            if calls.is_empty() {
                return self
                    .finish_turn(
                        message,
                        messages,
                        events,
                        emit,
                        round,
                        usage,
                        durable_attempt,
                        reasoning,
                        meter,
                        snapshot,
                    )
                    .await;
            }
            let planned = self
                .session
                .finish_provider_with_tools(
                    durable_attempt,
                    &message.content,
                    &reasoning,
                    meter,
                    &calls,
                )
                .map_err(super::session_err)?;
            self.execute_calls(planned, calls, &mut messages, events, &mut emit, usage)
                .await?;
        }
        self.abort_turn(abort_reason, events, emit, usage, *reached, snapshot)
            .await
    }

    async fn plan_first(
        &mut self,
        messages: &mut Vec<Message>,
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        if self.is_plan() || !self.plan_first || !self.session.needs_turn_plan() {
            return Ok(());
        }
        let objective = self.session.objective().map_err(super::session_err)?;
        let before = usage.clone();
        self.tools.set_read_only(true);
        self.schema = None;
        let drafted = plan::draft(self, &objective, usage).await;
        self.tools.set_read_only(self.is_plan());
        self.schema = None;
        self.seat_usage(workflow::Role::Plan, &before, usage, events, emit)?;
        let commands = self.verify.clone().unwrap_or_default();
        match drafted {
            Ok(text) => {
                self.session
                    .select_plan(&text, &commands)
                    .map_err(super::session_err)?;
                messages.push(Message::user(UserInput::from(plan::note(&text))));
                self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: "plan written before execution".to_string(),
                    },
                )?;
            }
            Err(error) => {
                self.session
                    .begin_without_plan(&commands)
                    .map_err(super::session_err)?;
                self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: format!(
                            "planning unavailable — executing without a written plan: {error}"
                        ),
                    },
                )?;
            }
        }
        let progress = self.work_progress()?;
        self.push_event(events, emit, Event::Work { progress })?;
        Ok(())
    }

    async fn scout(
        &mut self,
        messages: &mut Vec<Message>,
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        if self.seat_model(workflow::Role::Scout).is_none()
            || !self.session.needs_scout(self.is_plan())
        {
            return Ok(());
        }
        let objective = self.session.objective().map_err(super::session_err)?;
        let before = usage.clone();
        let brief = scout::brief(self, &objective, usage).await;
        self.seat_usage(workflow::Role::Scout, &before, usage, events, emit)?;
        let brief = match brief {
            Ok(brief) => brief,
            Err(error) => {
                return self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: format!("scout skipped — {error}"),
                    },
                );
            }
        };
        self.session.set_brief(&brief).map_err(super::session_err)?;
        let input = UserInput::from(scout::note(&brief));
        self.session.steer(&input).map_err(super::session_err)?;
        messages.push(Message::user(input));
        self.commit(messages);
        self.push_event(
            events,
            emit,
            Event::Note {
                text: "scout brief added to the conversation".to_string(),
            },
        )
    }

    fn begin_turn(&mut self, mut input: UserInput) -> Result<Vec<Message>> {
        if input.text.trim().is_empty() && input.images.is_empty() {
            return Err(Error::Config("the prompt is empty".to_string()));
        }
        let child_read_only = self.depth > 0 && self.tools.is_read_only();
        let read_only = self.is_plan() || child_read_only;
        if read_only != self.tools.is_read_only() {
            self.schema = None;
        }
        self.tools.set_read_only(read_only);
        if self.depth == 0 {
            self.tools.reset_edits();
            self.asked.retain(|grant| !matches!(grant, Grant::Edit(_)));
        }
        self.summary_failed = false;
        self.guard = super::guard::Guard::default();
        self.turn_started = crate::bus::now();
        let commands = self.verify.clone().unwrap_or_default();
        if self.is_plan() {
            self.session
                .prepare_plan_work(&input.text)
                .map_err(super::session_err)?;
        } else if self.is_goal() {
            if self
                .session
                .goal_objective()
                .map_err(super::session_err)?
                .is_none()
            {
                self.select_goal(&input.text, &commands, 0)?;
            }
            match self
                .session
                .prepare_turn_work(&input.text, &commands, self.plan_first)
                .map_err(super::session_err)?
            {
                TurnWork::Ready { accepted } => self.accepted = accepted,
                TurnWork::NeedsPlan => {}
            }
        } else {
            match self
                .session
                .prepare_turn_work(&input.text, &commands, self.plan_first)
                .map_err(super::session_err)?
            {
                TurnWork::Ready { accepted } => self.accepted = accepted,
                TurnWork::NeedsPlan => {}
            }
        }
        let mut harness = self.harness_note(&self.work_progress()?);
        if self.accepted {
            harness.push_str(
                "\n\nHarness: the selected plan is now accepted; /plan reject discards it.",
            );
        }
        input.text = decorate(&harness, &input.text);
        let queued = self
            .session
            .start_input(&input)
            .map_err(super::session_err)?;
        self.history.extend(queued.into_iter().map(Message::user));
        self.history.push(Message::user(input.clone()));
        let mut messages = Vec::with_capacity(self.history.len() + 2);
        messages.push(Message::system(self.system_prompt.clone()));
        messages.extend(self.history.iter().cloned());
        Ok(messages)
    }

    pub(super) fn round_abort(&self) -> Option<String> {
        self.budget.breach(self.spent_tokens, self.elapsed_secs())
    }

    fn harness_note(&self, progress: &WorkProgress) -> String {
        let mut note = harness_section(
            self.mode,
            self.verify.as_deref().unwrap_or_default(),
            !self.denies_network(),
            self.tools.has_sandbox(),
        );
        if progress.kind == WorkKind::Goal {
            note.push_str(&format!(
                "\n\nHarness objective (verbatim, this is what must be met):\n{}",
                progress.objective
            ));
        }
        if progress.kind != WorkKind::Single && !progress.ledger.is_empty() {
            note.push_str(
                "\n\nHarness ledger ([x] done, [/] this step, [ ] later, [!] blocked):\n",
            );
            note.push_str(&progress.ledger.join("\n"));
        }
        if self.seat_model(workflow::Role::Review).is_some() {
            note.push_str(
                "\n\nHarness: a reviewer reads the change after the completion check and can send it back.",
            );
        }
        note
    }

    async fn prepare_round(
        &mut self,
        messages: &mut Vec<Message>,
        tools: &[ToolDefinition],
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        self.bus_heartbeat("working");
        self.settle_probe().await;
        self.take_mail(messages, events, emit)?;
        self.drain(events, emit);
        self.auto_compact(messages, tools, usage, events, emit)
            .await
    }

    fn seat_usage(
        &mut self,
        role: workflow::Role,
        before: &Usage,
        after: &Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        self.sync_usage(emit);
        let prompt = after.prompt_tokens.saturating_sub(before.prompt_tokens);
        let completion = after
            .completion_tokens
            .saturating_sub(before.completion_tokens);
        if prompt == 0 && completion == 0 {
            return Ok(());
        }
        let cached = after.cached_tokens.saturating_sub(before.cached_tokens);
        self.push_event(
            events,
            emit,
            Event::Usage {
                prompt_tokens: prompt,
                completion_tokens: completion,
                reasoning_tokens: after
                    .reasoning_tokens
                    .saturating_sub(before.reasoning_tokens),
                cached_tokens: cached,
                millis: None,
                prefix_hash: format!("{role:?}").to_lowercase(),
                cost: self.price_for(role).map(|price| {
                    price.cost(u64::from(prompt), u64::from(completion), u64::from(cached))
                }),
            },
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "records one provider usage event with its durable and live sinks"
    )]
    pub(super) fn record_usage(
        &mut self,
        assistant: &AssistantMessage,
        prefix_hash: String,
        role: workflow::Role,
        millis: Option<u64>,
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        usage.add(&assistant.usage);
        self.spent_tokens +=
            u64::from(assistant.usage.prompt_tokens + assistant.usage.completion_tokens);
        if assistant.usage.prompt_tokens > 0 || assistant.usage.completion_tokens > 0 {
            self.push_event(
                events,
                emit,
                Event::Usage {
                    prompt_tokens: assistant.usage.prompt_tokens,
                    completion_tokens: assistant.usage.completion_tokens,
                    reasoning_tokens: assistant.usage.reasoning_tokens,
                    cached_tokens: assistant.usage.cached_tokens,
                    millis,
                    prefix_hash,
                    cost: self.price_for(role).map(|price| {
                        price.cost(
                            u64::from(assistant.usage.prompt_tokens),
                            u64::from(assistant.usage.completion_tokens),
                            u64::from(assistant.usage.cached_tokens),
                        )
                    }),
                },
            )?;
        }
        Ok(())
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "one close path for a finished provider round"
    )]
    async fn finish_turn(
        &mut self,
        message: Message,
        messages: Vec<Message>,
        events: &mut Vec<Event>,
        mut emit: impl FnMut(&Event),
        round: usize,
        usage: &mut Usage,
        attempt: Attempt,
        reasoning: String,
        meter: Meter,
        snapshot: String,
    ) -> Result<Turn> {
        if message.content.trim().is_empty() {
            self.session
                .fail_provider(
                    attempt,
                    "assistant returned neither text nor a tool call",
                    meter.millis,
                    true,
                )
                .map_err(super::session_err)?;
            return Err(Error::provider(
                ProviderFailureKind::Malformed,
                "assistant returned neither text nor a tool call",
            ));
        }
        if self.is_plan() {
            let receipt = Receipt::new(Vec::new(), Vec::new(), snapshot, events);
            let context = verify::context(&receipt);
            self.session
                .finish_plan(
                    attempt,
                    &message.content,
                    &reasoning,
                    meter,
                    self.verify.as_deref().unwrap_or_default(),
                    &context,
                )
                .map_err(super::session_err)?;
            self.commit(&messages);
            let progress = self.work_progress()?;
            self.push_event(events, &mut emit, Event::Work { progress })?;
            self.push_event(events, &mut emit, Event::Receipt { receipt })?;
            self.push_event(
                events,
                &mut emit,
                Event::Note {
                    text: format!(
                        "plan saved to {}",
                        crate::plan::file(&self.workspace).display()
                    ),
                },
            )?;
            let text = message.content;
            self.push_event(events, &mut emit, Event::Assistant { text: text.clone() })?;
            return Ok(Turn {
                text,
                events: std::mem::take(events),
                rounds: round,
                usage: std::mem::take(usage),
                checks: Vec::new(),
                aborted: false,
            });
        }
        self.session
            .finish_provider(attempt, &message.content, &reasoning, meter)
            .map_err(super::session_err)?;
        let (text, checks, receipt) = if self.session.stage() == Some(Stage::Execute)
            && (self.is_work() || self.is_goal())
        {
            let (mut checks, mut running, unsettled) = self.await_jobs(events, &mut emit).await?;
            let before_children = self.spent_tokens;
            running.extend(self.await_children(events, &mut emit, usage).await?);
            if self.spent_tokens > before_children
                && let Some(reason) = self.round_abort()
            {
                return self
                    .abort_turn(Some(reason), events, emit, usage, round, snapshot)
                    .await;
            }
            if !running.is_empty() {
                let note = format!(
                    "[summon: background work still running — {} — wait for it with the job or child id, or kill it, before you report done]",
                    running.join("; ")
                );
                if self
                    .session
                    .defer(&note, unsettled)
                    .map_err(super::session_err)?
                {
                    self.commit(&messages);
                    let progress = self.work_progress()?;
                    self.push_event(events, &mut emit, Event::Work { progress })?;
                    self.push_event(
                        events,
                        &mut emit,
                        Event::Note {
                            text: format!(
                                "turn deferred — background work still running: {}",
                                running.join("; ")
                            ),
                        },
                    )?;
                    let text = message.content;
                    self.push_event(events, &mut emit, Event::Assistant { text: text.clone() })?;
                    return Ok(Turn {
                        text,
                        events: std::mem::take(events),
                        rounds: round,
                        usage: std::mem::take(usage),
                        checks: Vec::new(),
                        aborted: false,
                    });
                }
            }
            let killed = self.kill_spawned("the parent turn ended", usage).await?;
            if !killed.is_empty() {
                self.push_event(
                    events,
                    &mut emit,
                    Event::Note {
                        text: format!(
                            "stopped {} background child(ren) the turn left running",
                            killed.len()
                        ),
                    },
                )?;
            }
            let paths = self.session.enter_verify().map_err(super::session_err)?;
            checks.extend(self.run_verify(events, &mut emit).await?);
            let mut verdict = self.verdict(&mut checks, events, &mut emit)?;
            let mut rejected = None;
            if verdict.passed && self.seat_model(workflow::Role::Review).is_some() {
                self.session.enter_review().map_err(super::session_err)?;
                let objective = self.session.objective().map_err(super::session_err)?;
                let evidence = verify::summary(&paths, &checks);
                let before = usage.clone();
                let reviewed =
                    review::outcome(self, &objective, &message.content, &evidence, &paths, usage)
                        .await;
                self.seat_usage(workflow::Role::Review, &before, usage, events, &mut emit)?;
                match reviewed {
                    Ok(review) => {
                        self.push_event(
                            events,
                            &mut emit,
                            Event::Verify {
                                command: review.command.clone(),
                                passed: review.passed,
                            },
                        )?;
                        if !review.passed {
                            verdict.passed = false;
                            rejected = Some(review.output.clone());
                            self.push_event(
                                events,
                                &mut emit,
                                Event::Note {
                                    text: format!("review findings:\n{}", review.output),
                                },
                            )?;
                        }
                        checks.push(review);
                    }
                    Err(why) => {
                        let review = Outcome {
                            command: review::CHECK.to_string(),
                            passed: false,
                            output: why,
                        };
                        self.push_event(
                            events,
                            &mut emit,
                            Event::Verify {
                                command: review.command.clone(),
                                passed: false,
                            },
                        )?;
                        let why = review.output.clone();
                        verdict.passed = false;
                        rejected = Some(why.clone());
                        checks.push(review);
                        self.push_event(
                            events,
                            &mut emit,
                            Event::Note {
                                text: format!("review unavailable — {why}"),
                            },
                        )?;
                    }
                }
            }
            let receipt = Receipt::new(paths, checks.clone(), snapshot, events);
            let decision = if verdict.passed {
                if let Some(objective) =
                    self.session.goal_objective().map_err(super::session_err)?
                {
                    let before = usage.clone();
                    let decision =
                        goal::decision(self, &objective, &message.content, &receipt.text, usage)
                            .await;
                    self.seat_usage(workflow::Role::Judge, &before, usage, events, &mut emit)?;
                    decision
                } else {
                    Decision::Automatic
                }
            } else {
                Decision::Automatic
            };
            if verdict.passed {
                verdict = self.verdict(&mut checks, events, &mut emit)?;
            }
            let receipt = Receipt::new(receipt.changes, checks.clone(), receipt.snapshot, events);
            let context = verify::context(&receipt);
            match rejected {
                Some(findings) => self
                    .session
                    .commit_review(&findings, Some(&context))
                    .map_err(super::session_err)?,
                None => self
                    .session
                    .commit_verdict(verdict, decision, Some(&context))
                    .map_err(super::session_err)?,
            }
            (message.content, checks, Some(receipt))
        } else {
            self.session.close().map_err(super::session_err)?;
            (message.content, Vec::new(), None)
        };
        self.commit(&messages);
        let progress = self.work_progress()?;
        self.push_event(events, &mut emit, Event::Work { progress })?;
        if let Some(receipt) = receipt {
            self.push_event(events, &mut emit, Event::Receipt { receipt })?;
        }
        let event = Event::Assistant { text: text.clone() };
        self.push_event(events, &mut emit, event)?;
        Ok(Turn {
            text,
            events: std::mem::take(events),
            rounds: round,
            usage: std::mem::take(usage),
            checks,
            aborted: false,
        })
    }

    fn verdict(
        &mut self,
        checks: &mut Vec<Outcome>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Verdict> {
        let verdict = self
            .session
            .prepare_verdict(checks.iter().all(|check| check.passed))
            .map_err(super::session_err)?;
        if let Some(output) = &verdict.diagnostic {
            let command = "workspace candidate unchanged".to_string();
            self.push_event(
                events,
                emit,
                Event::Verify {
                    command: command.clone(),
                    passed: false,
                },
            )?;
            checks.push(Outcome {
                command,
                passed: false,
                output: output.clone(),
            });
        }
        Ok(verdict)
    }

    async fn approve(
        &mut self,
        detail: String,
        persist: bool,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<GrantAnswer> {
        let Some(shared) = self.steer_shared.clone() else {
            return Ok(GrantAnswer::Deny);
        };
        let detail: String = detail.chars().take(160).collect();
        self.push_event(
            events,
            emit,
            Event::Approval {
                detail: Some(detail),
                persist,
            },
        )?;
        let deadline = Instant::now() + Duration::from_secs(600);
        let decision = loop {
            if Instant::now() >= deadline {
                break GrantAnswer::Deny;
            }
            let answer = shared.lock().ok().and_then(|mut queue| queue.pop_front());
            if let Some(input) = answer {
                break match parse_decision(input.text.trim(), persist) {
                    Some(decision) => decision,
                    None => {
                        self.steer.push_front(input);
                        GrantAnswer::Deny
                    }
                };
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
        };
        self.push_event(
            events,
            emit,
            Event::Approval {
                detail: None,
                persist: false,
            },
        )?;
        Ok(decision)
    }

    async fn with_grant(
        &mut self,
        mut output: Result<ToolOutput>,
        call: &ToolCall,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Result<ToolOutput>> {
        for _ in 0..4 {
            let Some(grant) = needed_grant(&output) else {
                break;
            };
            if self.tools.is_read_only() && !matches!(grant, Grant::Network) {
                break;
            }
            let persist = matches!(grant, Grant::Write(_));
            let question = grant.prompt();
            if self.asked.contains(&grant) {
                break;
            }
            if !self.manual
                && !matches!(
                    grant,
                    Grant::Guarded(_) | Grant::Command(_) | Grant::Danger(_) | Grant::Edit(_)
                )
            {
                self.apply_grant(&grant, false, events, emit)?;
                output = self.tools.execute_outcome(call).await;
                continue;
            }
            if let Grant::Danger(command) = &grant
                && !self.manual
            {
                let reason = crate::policy::danger(command, &self.workspace)
                    .unwrap_or("it looked destructive");
                match self.review(command, reason).await {
                    super::approval::Verdict::Allow => {
                        self.push_event(
                            events,
                            emit,
                            Event::Note {
                                text: format!("reviewer allowed: {command}"),
                            },
                        )?;
                        self.tools.approve_once();
                        output = self.tools.execute_outcome(call).await;
                        continue;
                    }
                    super::approval::Verdict::Block(why) => {
                        output = Ok(blocked(command, &why));
                        self.push_event(
                            events,
                            emit,
                            Event::Note {
                                text: format!("reviewer blocked {command}: {why}"),
                            },
                        )?;
                        break;
                    }
                    super::approval::Verdict::Ask(why) if self.steer_shared.is_none() => {
                        output = Ok(blocked(command, &why));
                        break;
                    }
                    super::approval::Verdict::Ask(_) => {}
                }
            }
            if let Grant::Edit(setting) = &grant
                && self.edit_requested(setting).await
            {
                self.apply_grant(&grant, false, events, emit)?;
                output = self.tools.execute_outcome(call).await;
                continue;
            }
            if self.steer_shared.is_none() {
                break;
            }
            self.asked.push(grant.clone());
            match self.approve(question, persist, events, emit).await? {
                GrantAnswer::Deny => break,
                GrantAnswer::Allow => self.apply_grant(&grant, false, events, emit)?,
                GrantAnswer::Always => self.apply_grant(&grant, true, events, emit)?,
            }
            output = self.tools.execute_outcome(call).await;
        }
        Ok(output)
    }

    fn apply_grant(
        &mut self,
        grant: &Grant,
        standing: bool,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        match grant {
            Grant::Write(path) | Grant::Guarded(path) => {
                self.allow_path(&path.to_string_lossy())?;
                let mut text = format!("writes allowed under {} for this session", path.display());
                if standing {
                    match crate::config::save_sandbox_write(
                        &crate::config::path()?,
                        &crate::config::sandbox_entry(path),
                    ) {
                        Ok(()) => {
                            text = format!(
                                "writes allowed under {} and saved to [sandbox] write",
                                path.display()
                            );
                        }
                        Err(error) => {
                            text = format!("{text}; standing grant not saved: {error}");
                        }
                    }
                }
                self.push_event(events, emit, Event::Note { text })?;
            }
            Grant::Danger(_) => self.tools.approve_once(),
            Grant::Edit(setting) => {
                self.tools.allow_edit(setting);
                self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: format!("{setting} allowed for this task"),
                    },
                )?;
            }
            Grant::Command(command) => {
                self.tools.approve_once();
                self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: format!("running this command without the sandbox: {command}"),
                    },
                )?;
            }
            Grant::Network => {
                self.set_network(true)?;
                self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: "network allowed for this session".to_string(),
                    },
                )?;
            }
        }
        Ok(())
    }

    async fn execute_calls(
        &mut self,
        planned: Vec<Call>,
        calls: Vec<ToolCall>,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<()> {
        for call in &calls {
            let event = Event::ToolCall {
                id: call.id.clone(),
                name: call.function.name.clone(),
                arguments: call.function.arguments.clone(),
            };
            self.push_event(events, emit, event)?;
        }
        let (planned, calls) = self.screen(planned, calls, messages, events, emit).await?;
        if calls.is_empty() {
            self.commit(messages);
            return Ok(());
        }
        let outputs = if calls.iter().all(|call| call.function.name == "task") {
            self.tasks_execute(&calls, events, emit)
                .await
                .into_iter()
                .map(|result| match result {
                    Ok(execution) => (
                        Ok(execution.output),
                        execution.millis,
                        Some(execution.usage),
                    ),
                    Err(error) => (Err(error), 0, None),
                })
                .collect()
        } else if calls.iter().all(|call| readonly(&call.function.name)) {
            let tools = &self.tools;
            futures::future::join_all(calls.iter().map(|call| async move {
                let started = Instant::now();
                (
                    tools.execute_outcome(call).await,
                    super::elapsed_millis(started),
                    None,
                )
            }))
            .await
        } else {
            for (planned, call) in planned.into_iter().zip(calls) {
                let started = Instant::now();
                let (output, child_usage) = if call.function.name == "task" {
                    match self.task_execute(&call, events, emit).await {
                        Ok(execution) => (Ok(execution.output), Some(execution.usage)),
                        Err(error) => (Err(error), None),
                    }
                } else if let Some(hub) =
                    self.mcp.clone().filter(|hub| hub.owns(&call.function.name))
                {
                    (
                        hub.call(
                            &call.function.name,
                            &call.function.arguments,
                            self.tools.is_read_only(),
                        )
                        .await,
                        None,
                    )
                } else if is_bus_tool(&call.function.name) {
                    (
                        self.bus_execute(&call).map(|text| ToolOutput {
                            text,
                            success: true,
                            mutated: false,
                            grant: None,
                        }),
                        None,
                    )
                } else {
                    let approval = self.manual && call.function.name == "run_command";
                    let denied = if approval {
                        let detail =
                            serde_json::from_str::<serde_json::Value>(&call.function.arguments)
                                .ok()
                                .and_then(|value| {
                                    value
                                        .get("command")
                                        .and_then(serde_json::Value::as_str)
                                        .map(str::to_string)
                                })
                                .unwrap_or_else(|| call.function.name.clone());
                        if self.approve(detail, false, events, emit).await? != GrantAnswer::Deny {
                            self.tools.approve_once();
                            None
                        } else {
                            Some(ToolOutput {
                                text: "summon: the user denied this command".to_string(),
                                success: false,
                                mutated: false,
                                grant: None,
                            })
                        }
                    } else {
                        None
                    };
                    (
                        if let Some(denied) = denied {
                            Ok(denied)
                        } else {
                            self.with_grant(
                                self.tools.execute_outcome(&call).await,
                                &call,
                                events,
                                emit,
                            )
                            .await?
                        },
                        None,
                    )
                };
                if let Some(child_usage) = child_usage {
                    usage.add(&child_usage);
                }
                self.finish_call(
                    planned,
                    call,
                    (output, super::elapsed_millis(started)),
                    messages,
                    events,
                    emit,
                )
                .await?;
            }
            self.commit(messages);
            return Ok(());
        };
        for ((planned, call), (output, millis, child_usage)) in
            planned.into_iter().zip(calls).zip(outputs)
        {
            if let Some(child_usage) = child_usage {
                usage.add(&child_usage);
            }
            self.finish_call(planned, call, (output, millis), messages, events, emit)
                .await?;
        }
        self.commit(messages);
        Ok(())
    }

    pub(super) async fn finish_call(
        &mut self,
        planned: Call,
        call: ToolCall,
        result: (Result<ToolOutput>, u64),
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let (output, millis) = result;
        let (success, mutated, text) = match output {
            Ok(output) => (output.success, output.mutated, output.text),
            Err(error) => (false, false, error.to_string()),
        };
        let text = self.tool_text(&call, success, text).await;
        self.session
            .finish_tools(&[(planned, text.as_str(), success, millis)])
            .map_err(super::session_err)?;
        let event = Event::ToolResult {
            id: call.id.clone(),
            success,
            mutated,
            output: text.clone(),
            millis: Some(millis),
        };
        self.push_event(events, emit, event)?;
        messages.push(Message::tool(call.id, text));
        if success
            && call.function.name == "todo"
            && let Ok(todos) = crate::tools::todos_from_args(&call.function.arguments)
        {
            let items: Vec<(String, crate::durable::work::StepState)> = todos
                .iter()
                .map(|todo| {
                    (
                        todo.text.clone(),
                        match todo.status {
                            crate::tools::TodoStatus::Done => {
                                crate::durable::work::StepState::Completed
                            }
                            crate::tools::TodoStatus::Active => {
                                crate::durable::work::StepState::Active
                            }
                            crate::tools::TodoStatus::Pending => {
                                crate::durable::work::StepState::Pending
                            }
                        },
                    )
                })
                .collect();
            if self.session.apply_todos(&items).is_ok()
                && let Ok(progress) = self.work_progress()
            {
                self.push_event(events, emit, Event::Work { progress })?;
            }
        }
        Ok(())
    }

    async fn await_jobs(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<(Vec<Outcome>, Vec<String>, Vec<String>)> {
        let mut outcomes: Vec<(String, Outcome)> = Vec::new();
        let mut running = Vec::new();
        let mut unsettled = Vec::new();
        let session = self.session.id().to_string();
        let mut owned: BTreeMap<String, jobs::Record> = BTreeMap::new();
        for id in self.session.jobs() {
            if let Ok(record) = jobs::load(&self.workspace, &id) {
                owned.insert(record.id.clone(), record);
            }
        }
        for record in jobs::since(&self.workspace, self.turn_started, Some(&session)) {
            owned.insert(record.id.clone(), record);
        }
        for record in owned.into_values() {
            let label = format!("job {} `{}`", record.id, record.command);
            let started = Instant::now();
            let mut wait = 1_u64;
            let mut announced = false;
            let status = loop {
                let status = jobs::status(&self.workspace, &record.id)?;
                if !status.running || started.elapsed().as_secs() >= JOB_WAIT_SECS {
                    break status;
                }
                if !announced {
                    announced = true;
                    self.push_event(
                        events,
                        emit,
                        Event::Note {
                            text: format!("waiting for background {label}"),
                        },
                    )?;
                }
                tokio::time::sleep(Duration::from_secs(wait)).await;
                wait = (wait * 2).min(10);
            };
            if status.killed {
                continue;
            }
            if status.running {
                running.push(label.clone());
                unsettled.push(record.id.clone());
            }
            let outcome = Outcome {
                command: label,
                passed: status.exit_code == Some(0),
                output: if status.running {
                    format!(
                        "still running after {JOB_WAIT_SECS}s — wait for it or kill it\n{}",
                        status.text
                    )
                } else {
                    status.text
                },
            };
            match outcomes
                .iter_mut()
                .find(|(command, _)| *command == record.command)
            {
                Some((_, latest)) => *latest = outcome,
                None => outcomes.push((record.command, outcome)),
            }
        }
        for (_, outcome) in &outcomes {
            self.push_event(
                events,
                emit,
                Event::Verify {
                    command: outcome.command.clone(),
                    passed: outcome.passed,
                },
            )?;
        }
        Ok((
            outcomes.into_iter().map(|(_, outcome)| outcome).collect(),
            running,
            unsettled,
        ))
    }

    async fn run_verify(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Vec<Outcome>> {
        let declared = self
            .session
            .completion_checks()
            .map_err(super::session_err)?;
        let mut checks = Vec::new();
        for check in declared {
            let command = check.command.clone();
            let outcome = match &check.observation {
                Some(Observation::File(path)) => match self.tools.exists(path.as_str()) {
                    Ok(true) => Outcome {
                        command,
                        passed: true,
                        output: format!("file `{}` exists", path.as_str()),
                    },
                    Ok(false) => Outcome {
                        command,
                        passed: false,
                        output: format!("file `{}` does not exist", path.as_str()),
                    },
                    Err(error) => Outcome {
                        command,
                        passed: false,
                        output: error.to_string(),
                    },
                },
                Some(Observation::Stdout(expected)) => {
                    let result = self.tools.execute_outcome(&verify_call(&command)).await;
                    let mut outcome = verify::outcome(&command, &result);
                    if outcome.passed && !outcome.output.contains(expected.as_str()) {
                        outcome.passed = false;
                        outcome.output.push_str(&format!(
                            "\nexpected stdout to contain `{}`",
                            expected.as_str()
                        ));
                    }
                    outcome
                }
                None => {
                    let result = self.tools.execute_outcome(&verify_call(&command)).await;
                    verify::outcome(&command, &result)
                }
            };
            self.push_event(
                events,
                emit,
                Event::Verify {
                    command: outcome.command.clone(),
                    passed: outcome.passed,
                },
            )?;
            let passed = outcome.passed;
            checks.push(outcome);
            if !passed {
                break;
            }
        }
        Ok(checks)
    }

    async fn abort_turn(
        &mut self,
        abort_reason: Option<String>,
        events: &mut Vec<Event>,
        mut emit: impl FnMut(&Event),
        usage: &mut Usage,
        reached: usize,
        snapshot: String,
    ) -> Result<Turn> {
        let reason = abort_reason.unwrap_or_else(|| "turn aborted".to_string());
        self.push_event(
            events,
            &mut emit,
            Event::Aborted {
                reason: reason.clone(),
            },
        )?;
        let receipt = Receipt::aborted(snapshot, events);
        self.push_event(
            events,
            &mut emit,
            Event::Receipt {
                receipt: receipt.clone(),
            },
        )?;
        drop(self.kill_spawned(&reason, usage).await?);
        self.tools.stop_jobs()?;
        self.session
            .abort(&reason, Some(&receipt.text))
            .map_err(super::session_err)?;
        self.sync_usage(&mut emit);
        self.hydrate();
        Ok(Turn {
            text: reason,
            events: std::mem::take(events),
            rounds: reached,
            usage: std::mem::take(usage),
            checks: Vec::new(),
            aborted: true,
        })
    }

    pub(super) fn commit(&mut self, _messages: &[Message]) {
        self.hydrate();
    }

    pub(super) fn hydrate(&mut self) {
        self.history = self.session.replay().history;
    }

    pub(super) fn sync_usage(&mut self, emit: &mut impl FnMut(&Event)) {
        let (spent, tokens) = self.session.usage();
        self.spent_tokens = spent;
        emit(&Event::Unreported { tokens });
    }

    pub(super) fn push_event(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        event: Event,
    ) -> Result<()> {
        emit(&event);
        events.push(event);
        Ok(())
    }
}

fn request_chars(messages: &[Message], tools: &[ToolDefinition]) -> usize {
    messages
        .iter()
        .map(super::compaction::message_chars)
        .sum::<usize>()
        + serde_json::to_string(tools).map_or(0, |schema| schema.len())
}

fn verify_call(command: &str) -> ToolCall {
    ToolCall {
        continuation: None,
        id: "completion-check".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": command,
                "timeout_secs": 600,
                "fresh": true
            })
            .to_string(),
        },
    }
}

fn blocked(command: &str, why: &str) -> ToolOutput {
    ToolOutput {
        text: format!(
            "summon: blocked `{command}` {why}. Take a safer approach, or ask the user to run it."
        ),
        success: false,
        mutated: false,
        grant: None,
    }
}
