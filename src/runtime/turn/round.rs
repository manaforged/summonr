use super::close::Answer;
use super::*;
use crate::tools::Builtin;

#[derive(Default)]
struct Pace {
    stalls: usize,
    continues: usize,
    planned: bool,
    acted: bool,
}

impl Pace {
    fn note_calls(&mut self, message: &Message) {
        self.stalls = 0;
        self.planned |= message
            .tool_calls
            .iter()
            .any(|call| Builtin::is(&call.function.name, Builtin::Todo));
        if acts(message) {
            self.acted = true;
            self.continues = 0;
        }
    }

    fn stalled(&self) -> bool {
        self.stalls >= crate::limits::limits().stall_rounds
    }
}

impl<P: Provider> Loop<P> {
    pub(super) async fn run_turn(
        &mut self,
        mut messages: Vec<Message>,
        pre: &mut Pretask,
        mut emit: impl FnMut(&Event),
        events: &mut Vec<Event>,
        usage: &mut Usage,
        reached: &mut usize,
    ) -> Result<Turn> {
        let abort;
        let mut pace = Pace::default();
        let tools = self
            .open_turn(&mut messages, usage, events, &mut emit)
            .await?;
        loop {
            while self.budget().max_tokens.is_some() && self.child_budget_reserved() > 0 {
                self.await_children(events, &mut emit, usage).await?;
            }
            *reached = reached.saturating_add(1);
            if let Some(reason) = self
                .open_round(&mut messages, tools.json.len(), usage, events, &mut emit)
                .await?
            {
                abort = reason;
                break;
            }
            let Some((assistant, request_chars, (attempt, meter, meta))) = self
                .round_call(&mut messages, &tools, events, &mut emit)
                .await?
            else {
                abort = self.budget_abort().unwrap_or_else(Abort::time_limit);
                break;
            };
            let (message, stop_reason, reasoning) =
                self.take_reply(assistant, request_chars, usage);
            let reply = (message, reasoning, attempt, meter, meta);
            let scope = (&mut messages, &mut *pre, &mut *events);
            if let Some(message) = self
                .take_round(reply, stop_reason, &mut pace, scope, &mut emit, usage)
                .await?
            {
                return self
                    .finish_turn(message, events, emit, *reached, usage, pre)
                    .await;
            }
        }
        self.abort_turn(abort, events, emit, usage, *reached, pre)
            .await
    }

    async fn take_round(
        &mut self,
        reply: (Message, String, Attempt, Meter, UsageMeta),
        stop_reason: StopReason,
        pace: &mut Pace,
        (messages, pre, events): (&mut Vec<Message>, &mut Pretask, &mut Vec<Event>),
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<Option<Message>> {
        let (message, reasoning, attempt, meter, meta) = reply;
        if !message.tool_calls.is_empty() {
            pace.note_calls(&message);
            let reply = (message, reasoning, attempt, meter, meta);
            self.continue_with(reply, messages, pre, events, emit, usage)
                .await?;
            return Ok(None);
        }
        if stop_reason == StopReason::Length {
            let mut steered = Vec::new();
            self.steer_length(
                &message,
                &reasoning,
                attempt,
                meter,
                meta,
                &mut steered,
                events,
                emit,
            )
            .await?;
            messages.push(message);
            messages.append(&mut steered);
            return Ok(None);
        }
        let reply = (message, reasoning, attempt, meter, meta);
        self.close_reply(reply, pace, (messages, pre, events), emit, usage)
            .await
    }

    async fn close_reply(
        &mut self,
        (message, reasoning, attempt, meter, meta): (Message, String, Attempt, Meter, UsageMeta),
        pace: &mut Pace,
        (messages, pre, events): (&mut Vec<Message>, &mut Pretask, &mut Vec<Event>),
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<Option<Message>> {
        let capped = pace.continues >= crate::limits::limits().turn_continues;
        let steered = self.steer_waiting();
        let tasks = if steered {
            None
        } else {
            self.open_tasks(pace)?
        };
        let holds = !capped && (steered || tasks.is_some());
        if !holds && message.content.trim().is_empty() {
            self.settle(pre, events, emit).await?;
            return self.reject_empty(attempt, meter.millis, events, emit);
        }
        let answer = Answer {
            attempt,
            text: &message.content,
            reasoning: &reasoning,
            meter,
            meta,
        };
        self.record_answer(answer, events, emit)?;
        if capped && (steered || tasks.is_some() || self.judge_due(pace)) {
            self.note_cap(pace, events, emit)?;
        }
        let note = if capped {
            None
        } else if holds {
            tasks
        } else {
            self.judge_reply(&message.content, pace, events, emit, usage)
                .await?
        };
        if !holds && note.is_none() {
            return Ok(Some(message));
        }
        self.keep_reply(message, messages, events, emit)?;
        if steered {
            let before = messages.len();
            self.take_mail(messages, events, emit)?;
            pace.continues += usize::from(messages.len() == before);
        }
        if let Some(note) = note {
            self.continue_turn(note, pace, messages, events, emit)?;
        }
        Ok(None)
    }

    fn note_cap(
        &mut self,
        pace: &Pace,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let text = format!(
            "the turn ended after {} continuations (turn_continues); send a message to keep going",
            pace.continues
        );
        self.push_event(events, emit, Event::Note { text })
    }

    fn continue_turn(
        &mut self,
        note: String,
        pace: &mut Pace,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        pace.stalls += 1;
        pace.continues += 1;
        self.inject(note, messages)?;
        let text = "work is still open; the turn continues".to_string();
        self.push_event(events, emit, Event::Note { text })
    }

    fn keep_reply(
        &mut self,
        message: Message,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        if !message.content.is_empty() {
            let text = message.content.clone();
            self.push_event(events, emit, Event::Assistant { text })?;
        }
        messages.push(message);
        Ok(())
    }

    fn open_tasks(&self, pace: &Pace) -> Result<Option<String>> {
        if pace.stalled() || self.session.stage() == Some(Stage::Plan) || !pace.planned {
            return Ok(None);
        }
        let todos = match self.session.work_progress() {
            Ok(progress) => progress.todos,
            Err(crate::durable::driver::DriverError::NoWork) => return Ok(None),
            Err(error) => return Err(Error::from(error)),
        };
        let open = todos
            .iter()
            .filter(|todo| todo.open())
            .map(crate::tools::Todo::line)
            .collect::<Vec<_>>();
        Ok((!open.is_empty()).then(|| format!("{OPEN_WORK}\n{}", open.join("\n"))))
    }

    fn judge_due(&self, pace: &Pace) -> bool {
        self.judge_turns
            && pace.acted
            && !pace.stalled()
            && !self.is_goal()
            && self.session.stage() != Some(Stage::Plan)
    }

    async fn judge_reply(
        &mut self,
        reply: &str,
        pace: &Pace,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<Option<String>> {
        if !self.judge_due(pace) {
            return Ok(None);
        }
        let Ok(objective) = self.session.objective() else {
            return Ok(None);
        };
        let evidence = goal::with_corrections(TURN_EVIDENCE, events);
        let decision = goal::judged(self, &objective, reply, &evidence, usage).await;
        self.sync_usage(emit);
        match decision {
            Ok(Decision::GoalContinue(reason)) => Ok(Some(format!("{UNFINISHED}\n{reason}"))),
            Ok(_) => Ok(None),
            Err(text) => {
                self.push_event(events, emit, Event::Note { text })?;
                Ok(None)
            }
        }
    }

    async fn continue_with(
        &mut self,
        reply: (Message, String, Attempt, Meter, UsageMeta),
        messages: &mut Vec<Message>,
        pre: &mut Pretask,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<()> {
        let (message, reasoning, durable_attempt, meter, meta) = reply;
        let calls = message.tool_calls.clone();
        let planned = self.session.finish_provider_with_tools(
            durable_attempt,
            &message.content,
            &reasoning,
            meter,
            &calls,
            meta,
        )?;
        messages.push(message);
        self.emit_records(events, emit)?;
        self.settle(pre, events, emit).await?;
        self.capture_deferred(events, emit)?;
        self.session.settle_base()?;
        self.execute_calls(planned, calls, messages, events, emit, usage)
            .await
    }

    async fn open_turn(
        &mut self,
        messages: &mut Vec<Message>,
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Arc<super::mail::Advertised>> {
        self.settle_mcp(events, emit).await?;
        let tools = self.advertised_tools();
        self.scout(messages, usage, events, emit).await?;
        Ok(tools)
    }

    async fn open_round(
        &mut self,
        messages: &mut Vec<Message>,
        tool_bytes: usize,
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Option<Abort>> {
        if let Some(abort) = self.budget_abort() {
            return Ok(Some(abort));
        }
        if let Err(error) = self
            .prepare_round(messages, tool_bytes, usage, events, emit)
            .await
        {
            return self.budget_abort().map(Some).ok_or(error);
        }
        Ok(self.budget_abort())
    }

    async fn round_call(
        &mut self,
        messages: &mut Vec<Message>,
        tools: &super::super::mail::Advertised,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Option<(AssistantMessage, usize, (Attempt, Meter, UsageMeta))>> {
        let role = crate::workflow::Role::Implement;
        let mut state = super::retry::RetryState::default();
        loop {
            let request = self.request_with(role, messages, tools);
            self.freeze_catalog();
            let options = CallOptions {
                stream: true,
                retry: RetryPolicy::Round,
            };
            let Missed { error, interrupted } = match self.call(role, request, options).await? {
                Ok(reply) => {
                    let open = reply
                        .open
                        .ok_or_else(|| Error::Config("no provider round is open".to_string()))?;
                    return Ok(Some((reply.message, reply.chars, open)));
                }
                Err(missed) => missed,
            };
            let error = match error {
                WaitError::Steered => {
                    messages.extend(interrupted);
                    let stopped = self.steered("");
                    if !stopped.is_empty() {
                        messages.push(Message::system(stopped));
                    }
                    self.emit_records(events, emit)?;
                    self.sync_usage(emit);
                    self.take_mail(messages, events, emit)?;
                    if self.round_abort().is_some() {
                        return Ok(None);
                    }
                    continue;
                }
                WaitError::Deadline(_) => None,
                WaitError::Provider(error) => {
                    messages.extend(interrupted);
                    Some(error)
                }
            };
            self.emit_records(events, emit)?;
            self.sync_usage(emit);
            let Some(error) = error else {
                return Ok(None);
            };
            if !self
                .provider_failed(error, &mut state, messages, tools, events, emit)
                .await?
            {
                return Ok(None);
            }
        }
    }

    fn take_reply(
        &mut self,
        assistant: AssistantMessage,
        request_chars: usize,
        usage: &mut Usage,
    ) -> (Message, StopReason, String) {
        self.summary_wait = self.summary_wait.saturating_sub(1);
        if assistant.usage.prompt_tokens > 0 {
            self.context_sample = Some((request_chars, assistant.usage.prompt_tokens));
        }
        usage.add(&assistant.usage);
        let stop_reason = assistant.stop_reason;
        let reasoning = assistant.reasoning_content.clone().unwrap_or_default();
        (assistant.into_message(), stop_reason, reasoning)
    }

    pub(super) async fn scout(
        &mut self,
        messages: &mut Vec<Message>,
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        if self.role_model(workflow::Role::Scout).is_none() || !self.session.needs_scout() {
            return Ok(());
        }
        let objective = self.session.objective()?;
        let brief = scout::brief(self, &objective, usage).await;
        self.sync_usage(emit);
        let brief = match brief {
            Ok(brief) => brief,
            Err(error) => {
                return self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: format!("scout skipped: {error}"),
                    },
                );
            }
        };
        self.session.set_brief(&brief)?;
        self.inject(scout::note(&brief), messages)?;
        self.push_event(
            events,
            emit,
            Event::Note {
                text: "scout brief added to the conversation".to_string(),
            },
        )
    }

    pub(super) fn begin_turn(
        &mut self,
        mut input: UserInput,
        trigger: Trigger,
    ) -> Result<Vec<Message>> {
        if input.text.trim().is_empty() && input.images.is_empty() {
            return Err(Error::Config("the prompt is empty".to_string()));
        }
        let read_only = self.session.depth() > 0 && self.tools.is_read_only();
        self.tools.set_read_only(read_only);
        self.summary_wait = 0;
        self.turn_started = summonr_core::clock::now();
        let commands = self.verify.clone().unwrap_or_default();
        self.tools.clear_wrote();
        self.tools.clear_detached();
        self.prepare_work(&input.text, &commands, trigger)?;
        if self.is_goal() {
            self.prefix_findings(&mut input);
        }
        let exited = self.finished_jobs().join("\n");
        let mut harness = self.harness_note(&self.work_progress()?);
        if !exited.is_empty() {
            harness = format!("{harness}\n\nHarness: {exited}");
        }
        if !self.note_sent(&harness) {
            if input.source.is_none() {
                input.source = Some(crate::provider::Source {
                    text: input.text.clone(),
                    images: input.images.len(),
                });
            }
            input.text = decorate(&harness, &input.text);
        }
        self.session.start_input(&input)?;
        let history = self.session.history();
        let mut messages = Vec::with_capacity(history.len() + 1);
        messages.push(Message::system(self.system_prompt.clone()));
        messages.extend(history.iter().cloned());
        Ok(messages)
    }

    fn prefix_findings(&mut self, input: &mut crate::provider::UserInput) {
        let Some(mailbox) = self.mailbox.as_mut() else {
            return;
        };
        let Ok(mail) = mailbox.poll() else {
            return;
        };
        let notes: Vec<String> = mail
            .into_iter()
            .map(|note| note.text)
            .filter(|text| !text.trim().is_empty())
            .collect();
        if notes.is_empty() {
            return;
        }
        input.text = format!("{}\n\n{}", notes.join("\n\n"), input.text);
    }

    fn prepare_work(&mut self, text: &str, commands: &[String], trigger: Trigger) -> Result<()> {
        let objective = match trigger {
            Trigger::User => text.to_string(),
            Trigger::Mail => self
                .session
                .objective()
                .unwrap_or_else(|_no_work| text.to_string()),
        };
        if self.is_goal() && self.session.goal_objective()?.is_none() {
            self.select_goal(&objective, commands)?;
        }
        self.session
            .prepare_turn_work(&objective, commands)
            .map_err(Error::from)
    }

    pub(in crate::runtime) fn round_abort(&self) -> Option<String> {
        self.budget_abort().map(|abort| abort.reason)
    }

    pub(in crate::runtime) fn budget_abort(&self) -> Option<Abort> {
        self.budget().breach(self.run_tokens(), self.elapsed_secs())
    }

    pub(super) async fn prepare_round(
        &mut self,
        messages: &mut Vec<Message>,
        tool_bytes: usize,
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        self.mail_heartbeat(crate::mail::Presence::Working);
        self.settle_probe().await;
        self.take_mail(messages, events, emit)?;
        self.drain(events, emit);
        if self
            .auto_compact(messages, tool_bytes, usage, events, emit)
            .await?
        {
            let note = self.harness_note(&self.work_progress()?);
            if !note.is_empty() {
                self.inject(note, messages)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "round_tests.rs"]
mod tests;

fn acts(message: &Message) -> bool {
    message
        .tool_calls
        .iter()
        .any(|call| Builtin::of(&call.function.name).is_some_and(|tool| !tool.readonly()))
}

const OPEN_WORK: &str = "[summon control: the reply had no tool call, but tasks are still open. Continue with the next one now. Mark a task done when it is finished, or blocked when only the user or an outside event can move it, and name who in its text.]\nOpen tasks:";

const UNFINISHED: &str = "[summon control: the reply had no tool call, but the judge found work you can still do without the user. Continue it now. If only the user can unblock what is left, say so in one line.]\nWhat is left:";

const TURN_EVIDENCE: &str = "Interactive turn. Reply done when nothing is left that the agent can do without the user, including when the only remaining step needs the user's approval. Reply continue only when the agent stopped with work it can still do; name that work.";
