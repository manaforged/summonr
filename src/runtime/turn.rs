use super::checkpoint::Pretask;
use super::compaction::{force_compact, repair_arguments};
use super::*;
use crate::ProviderFailureKind;
use crate::durable::driver::{Purpose, TurnWork, Verdict};
use crate::durable::id::{Attempt, Call};
use crate::durable::work::{Decision, Observation, Stage};
use crate::workflow;
use std::future::Future;
use std::time::Duration;

use crate::mail::{envelope, is_mail_tool};
use crate::policy::Grant;
use crate::prompt::harness_section;
use crate::provider::StopReason;
use crate::session::decorate;
use crate::tools::{ToolOutput, needed_grant, readonly};
use crate::{Receipt, WorkKind, WorkProgress, goal, jobs, plan, review, scout, verify};

use crate::limits::REPAIR_LIMIT;

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

mod calls;
mod checks;
mod finish;
mod grant;
mod retry;
mod round;
mod usage;

impl<P: Provider> Loop<P> {
    fn completion_request<'a>(
        &self,
        role: workflow::Role,
        messages: &'a [Message],
        tools: &'a [ToolDefinition],
    ) -> CompletionRequest<'a> {
        self.request(role, messages, tools, true)
    }

    pub(super) fn remaining_time(&self) -> Option<Duration> {
        let limit = crate::limits::seconds(self.budget.max_secs?);
        Some(limit.saturating_sub(crate::limits::seconds(
            self.session.elapsed_millis() / 1_000,
        )))
    }

    pub(super) async fn complete_once(
        &mut self,
        role: workflow::Role,
        request: &CompletionRequest<'_>,
    ) -> Result<std::result::Result<AssistantMessage, WaitError>> {
        if let Some(reason) = self.round_abort() {
            return Err(Error::Config(reason));
        }
        let chars = request_chars(
            request.messages,
            request.tools,
            self.seat(role).0.keeps_reasoning(),
        );
        let attempt = self
            .session
            .start_round(
                purpose_of(role),
                &request.model,
                super::exposure(request, chars),
            )
            .map_err(super::session_err)?;
        let started = Instant::now();
        match Self::wait(self.seat(role).0.complete(request), self.remaining_time()).await {
            Ok(message) => {
                let meter = super::meter(&message.usage, super::elapsed_millis(started));
                let meta = UsageMeta {
                    continuation: message.continuation.clone(),
                    prefix_hash: request.prefix_hash(),
                    cost: self
                        .price_for(role)
                        .map(|price| price.cost(meter.input, meter.output, meter.cached)),
                };
                self.session
                    .finish_provider(
                        attempt,
                        message.content.as_deref().unwrap_or(""),
                        message.reasoning_content.as_deref().unwrap_or(""),
                        meter,
                        meta,
                    )
                    .map_err(super::session_err)?;
                self.spent_tokens = self.spent_tokens.saturating_add(u64::from(
                    message
                        .usage
                        .prompt_tokens
                        .saturating_add(message.usage.completion_tokens),
                ));
                Ok(Ok(message))
            }
            Err(wait_error) => {
                let millis = wait_error.millis(started);
                let deadline = match wait_error {
                    WaitError::Deadline(remaining) => Some(remaining),
                    WaitError::Provider(_) => None,
                };
                let error = wait_error.error();
                self.session
                    .fail_provider(
                        attempt,
                        &error.to_string(),
                        millis,
                        deadline.is_some() || super::billable(&error),
                    )
                    .map_err(super::session_err)?;
                self.spent_tokens = self.session.spent_tokens();
                Ok(Err(deadline.map_or_else(
                    || WaitError::Provider(error),
                    WaitError::Deadline,
                )))
            }
        }
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
        let chars = request_chars(messages, tools, self.provider.keeps_reasoning());
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
        let messages = self.begin_turn(input.into())?;
        let mut pre = self.checkpoint();
        let mut events = Vec::new();
        let mut usage = Usage::default();
        let mut rounds = 0;
        let remaining = self.remaining_time();
        let before = self.session.elapsed_millis();
        let started = Instant::now();
        let turn = self.run_turn(
            messages,
            &mut pre,
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
        let accounted = self.session.elapsed_millis().saturating_sub(before);
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
                    UsageMeta::default(),
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
                    &mut pre,
                )
                .await
            }
        }
    }

    pub async fn turn_mail_with(&mut self, emit: impl FnMut(&Event)) -> Result<Turn> {
        if self.inbox.is_empty() {
            return Err(Error::Config("there is no pending peer mail".to_string()));
        }
        self.turn_with(
            "[summon control: respond to the pending peer mail in this Work]",
            emit,
        )
        .await
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

fn purpose_of(role: workflow::Role) -> Purpose {
    match role {
        workflow::Role::Implement => Purpose::Agent,
        workflow::Role::Summary => Purpose::Summary,
        workflow::Role::Judge => Purpose::Judge,
        workflow::Role::Review => Purpose::Review,
        workflow::Role::Scout => Purpose::Scout,
        workflow::Role::Plan => Purpose::Plan,
    }
}

fn request_chars(messages: &[Message], tools: &[ToolDefinition], reasoning_on_wire: bool) -> usize {
    messages
        .iter()
        .map(|message| super::compaction::message_chars(message, reasoning_on_wire))
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
            "summonr: blocked `{command}` — {why}. Take a safer approach, or ask the user to run it."
        ),
        success: false,
        mutated: false,
        grant: None,
    }
}
