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
            while self.budget().max_tokens.is_some() && self.child_budget_reserved() > 0 {
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
            let Some((assistant, request_chars, (durable_attempt, meter, meta))) = self
                .round_call(&mut messages, &tools, events, &mut emit)
                .await?
            else {
                abort = self.budget_abort().unwrap_or_else(Abort::time_limit);
                break;
            };
            let (message, stop_reason, reasoning) =
                self.take_reply(assistant, request_chars, usage);
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
                self.session.finish_provider(
                    durable_attempt,
                    &message.content,
                    &reasoning,
                    meter,
                    meta,
                )?;
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

    pub(super) fn begin_turn(&mut self, mut input: UserInput) -> Result<Vec<Message>> {
        if input.text.trim().is_empty() && input.images.is_empty() {
            return Err(Error::Config("the prompt is empty".to_string()));
        }
        let read_only = self.session.depth() > 0 && self.tools.is_read_only();
        self.tools.set_read_only(read_only);
        self.summary_wait = 0;
        self.turn_started = crate::mail::now();
        let commands = self.verify.clone().unwrap_or_default();
        self.tools.clear_wrote();
        self.prepare_work(&input.text, &commands)?;
        let harness = self.harness_note(&self.work_progress()?);
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

    fn prepare_work(&mut self, text: &str, commands: &[String]) -> Result<()> {
        if self.is_goal() && self.session.goal_objective()?.is_none() {
            self.select_goal(text, commands)?;
        }
        self.session
            .prepare_turn_work(text, commands)
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
