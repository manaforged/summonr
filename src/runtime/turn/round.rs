use super::*;

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
        let tools = self
            .open_turn(&mut messages, usage, events, &mut emit)
            .await?;

        loop {
            while self.budget.max_tokens.is_some() && self.child_budget_reserved() > 0 {
                self.await_children(events, &mut emit, usage).await?;
            }
            *reached = reached.saturating_add(1);
            let round = *reached;
            if let Some(reason) = self
                .open_round(&mut messages, tools.json.len(), usage, events, &mut emit)
                .await?
            {
                abort = reason;
                break;
            }
            let Some((assistant, prefix_hash, request_chars, durable_attempt, live_millis)) = self
                .complete_retry(&mut messages, &tools, events, &mut emit)
                .await?
            else {
                abort = self.budget_abort().unwrap_or_else(Abort::time_limit);
                break;
            };
            let (message, stop_reason, meter, reasoning, meta) =
                self.take_reply(assistant, prefix_hash, request_chars, live_millis, usage);
            if stop_reason == StopReason::Length && message.tool_calls.is_empty() {
                let mut steered = Vec::new();
                self.steer_length(
                    &message,
                    &reasoning,
                    durable_attempt,
                    meter,
                    meta,
                    &mut steered,
                    events,
                    &mut emit,
                )
                .await?;
                messages.push(message);
                messages.append(&mut steered);
                continue;
            }
            if message.tool_calls.is_empty() && self.steer_waiting() {
                self.session
                    .finish_provider(durable_attempt, &message.content, &reasoning, meter, meta)
                    .map_err(super::session_err)?;
                self.emit_records(events, &mut emit)?;
                if !message.content.is_empty() {
                    let text = message.content.clone();
                    self.push_event(events, &mut emit, Event::Assistant { text })?;
                }
                messages.push(message);
                continue;
            }
            if message.tool_calls.is_empty() {
                return self
                    .finish_turn(
                        message,
                        events,
                        emit,
                        round,
                        usage,
                        durable_attempt,
                        reasoning,
                        meter,
                        meta,
                        pre,
                    )
                    .await;
            }
            self.continue_with(
                (message, reasoning, durable_attempt, meter, meta),
                &mut messages,
                pre,
                events,
                &mut emit,
                usage,
            )
            .await?;
        }
        self.abort_turn(abort, events, emit, usage, *reached, pre)
            .await
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
        let planned = self
            .session
            .finish_provider_with_tools(
                durable_attempt,
                &message.content,
                &reasoning,
                meter,
                &calls,
                meta,
            )
            .map_err(super::session_err)?;
        messages.push(message);
        self.emit_records(events, emit)?;
        self.settle(pre, events, emit).await?;
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

    fn take_reply(
        &mut self,
        assistant: AssistantMessage,
        prefix_hash: String,
        request_chars: usize,
        live_millis: u64,
        usage: &mut Usage,
    ) -> (Message, StopReason, Meter, String, UsageMeta) {
        self.summary_wait = self.summary_wait.saturating_sub(1);
        if assistant.usage.prompt_tokens > 0 {
            self.context_sample = Some((request_chars, assistant.usage.prompt_tokens));
        }
        let role = self.round_role();
        let meta = self.record_usage(&assistant, prefix_hash, role, usage);
        let stop_reason = assistant.stop_reason;
        let meter = super::meter(&assistant.usage, live_millis);
        let reasoning = assistant.reasoning_content.clone().unwrap_or_default();
        (
            assistant.into_message(),
            stop_reason,
            meter,
            reasoning,
            meta,
        )
    }

    pub(super) async fn scout(
        &mut self,
        messages: &mut Vec<Message>,
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        if self.seat_model(workflow::Role::Scout).is_none() || !self.session.needs_scout() {
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
                        text: format!("scout skipped: {error}"),
                    },
                );
            }
        };
        self.session.set_brief(&brief).map_err(super::session_err)?;
        self.inject(scout::note(&brief), messages)?;
        self.push_event(
            events,
            emit,
            Event::Note {
                text: "scout brief added to the conversation".to_string(),
            },
        )
    }

    pub(super) fn begin_turn(&mut self, mut input: UserInput) -> Result<Vec<Message>> {
        if input.text.trim().is_empty() && input.images.is_empty() {
            return Err(Error::Config("the prompt is empty".to_string()));
        }
        let read_only = self.depth > 0 && self.tools.is_read_only();
        self.tools.set_read_only(read_only);
        self.summary_wait = 0;
        self.turn_started = crate::mail::now();
        let commands = self.verify.clone().unwrap_or_default();
        self.prepare_work(&input.text, &commands)?;
        let harness = self.harness_note(&self.work_progress()?);
        if !self.note_sent(&harness) {
            input.text = decorate(&harness, &input.text);
        }
        self.session
            .start_input(&input)
            .map_err(super::session_err)?;
        let history = self.session.history();
        let mut messages = Vec::with_capacity(history.len() + 1);
        messages.push(Message::system(self.system_prompt.clone()));
        messages.extend(history.iter().cloned());
        Ok(messages)
    }

    fn prepare_work(&mut self, text: &str, commands: &[String]) -> Result<()> {
        if self.is_goal()
            && self
                .session
                .goal_objective()
                .map_err(super::session_err)?
                .is_none()
        {
            self.select_goal(text, commands)?;
        }
        self.session
            .prepare_turn_work(text, commands)
            .map_err(super::session_err)
    }

    pub(in crate::runtime) fn round_abort(&self) -> Option<String> {
        self.budget_abort().map(|abort| abort.reason)
    }

    pub(in crate::runtime) fn budget_abort(&self) -> Option<Abort> {
        self.budget.breach(self.run_tokens(), self.elapsed_secs())
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
        self.auto_compact(messages, tool_bytes, usage, events, emit)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "round_tests.rs"]
mod tests;
