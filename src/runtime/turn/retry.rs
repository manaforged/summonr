use super::*;

impl<P: Provider> Loop<P> {
    pub(super) fn take_mail(
        &mut self,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        while let Some(input) = self.steer.pop_front() {
            self.session.steer(&input).map_err(super::session_err)?;
            messages.push(Message::user(input));
        }
        if let Some(shared) = &self.steer_shared
            && let Ok(mut queue) = shared.lock()
        {
            while let Some(input) = queue.pop_front() {
                self.session.steer(&input).map_err(super::session_err)?;
                messages.push(Message::user(input));
            }
        }
        let (mail, overflow) = self.take_bus_mail(5)?;
        for mail in mail {
            let input = UserInput::from(envelope(&mail));
            self.session.steer(&input).map_err(super::session_err)?;
            messages.push(Message::user(input));
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
        }
        Ok(())
    }

    pub(super) async fn complete_retry(
        &mut self,
        messages: &mut Vec<Message>,
        tools: &[ToolDefinition],
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Option<(AssistantMessage, String, usize, Attempt, u64)>> {
        let role = self.round_role();
        let mut retried = false;
        let mut repairs = 0_usize;
        let mut overflows = 0_usize;
        let mut usage = Usage::default();
        loop {
            let request = self.completion_request(role, messages, tools);
            let prefix_hash = request.prefix_hash();
            self.freeze_catalog();
            let request_chars = request_chars(messages, tools, self.provider.keeps_reasoning());
            let durable_attempt = self
                .session
                .start_provider(&request.model, super::exposure(&request, request_chars))
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
                    self.session.partial = crate::provider::Partial::default();
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
                    self.emit_records(events, emit)?;
                    self.sync_usage(emit);
                    return Ok(None);
                }
                Err(WaitError::Provider(error)) => {
                    let failure = error.provider_failure();
                    let retryable = !retried
                        && failure.is_some_and(|failure| {
                            failure.kind == ProviderFailureKind::Authentication
                                || (failure.partial_output
                                    && failure.status.is_none()
                                    && matches!(
                                        failure.kind,
                                        ProviderFailureKind::Transport
                                            | ProviderFailureKind::Server
                                    ))
                        });
                    if retryable {
                        self.session.partial = crate::provider::Partial::default();
                    }
                    self.session
                        .fail_provider(
                            durable_attempt,
                            &error.to_string(),
                            super::elapsed_millis(started),
                            super::billable(&error),
                        )
                        .map_err(super::session_err)?;
                    self.emit_records(events, emit)?;
                    self.sync_usage(emit);
                    if self.round_abort().is_some() {
                        return Ok(None);
                    }
                    let text = error.to_string();
                    if failure.is_some_and(|failure| {
                        failure.kind == ProviderFailureKind::MalformedToolCall
                    }) {
                        if repairs >= REPAIR_LIMIT {
                            return Err(error);
                        }
                        repairs += 1;
                        let edits = repair_arguments(&messages[1..]);
                        self.rewrite(messages, edits)?;
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
                        if overflows >= REPAIR_LIMIT {
                            return Err(error);
                        }
                        overflows += 1;
                        if let Some(cap) = crate::provider::input_cap(&text) {
                            self.set_context_window(Some(cap));
                        }
                        self.auto_compact(messages, tools, &mut usage, events, emit)
                            .await?;
                        let edits = force_compact(&messages[1..]);
                        self.rewrite(messages, edits)?;
                        self.tools.forget_reads();
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
                    if !retryable {
                        if self.seats.contains_key(&role) {
                            return Err(error);
                        }
                        let Some((provider, model)) = self.fallbacks.pop_front() else {
                            return Err(error);
                        };
                        let from = std::mem::replace(&mut self.model, model);
                        self.provider = provider;
                        self.context_sample = None;
                        retried = false;
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
                    retried = true;
                    self.push_event(
                        events,
                        emit,
                        Event::Note {
                            text: format!(
                                "stream failed — retrying once: {}",
                                text.lines().next().unwrap_or("unknown")
                            ),
                        },
                    )?;
                }
            }
        }
    }
}
