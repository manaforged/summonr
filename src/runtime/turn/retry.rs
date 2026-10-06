use super::*;

const MAIL_PER_ROUND: usize = 5;

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
        let (mail, overflow) = self.take_peer_mail(MAIL_PER_ROUND)?;
        for mail in mail {
            let input = UserInput::from(envelope(&mail));
            self.session.steer(&input).map_err(super::session_err)?;
            messages.push(Message::user(input));
            self.push_event(
                events,
                emit,
                Event::Mail {
                    from: mail.sender().to_string(),
                    text: mail.text,
                },
            )?;
        }
        if overflow > 0 {
            let note = format!("[summon mail: {overflow} more message(s) waiting in the inbox]");
            let input = UserInput::from(note);
            self.session.steer(&input).map_err(super::session_err)?;
            messages.push(Message::user(input));
        }
        Ok(())
    }

    pub(super) async fn complete_retry(
        &mut self,
        messages: &mut Vec<Message>,
        tools: &super::super::mail::Advertised,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Option<(AssistantMessage, String, usize, Attempt, u64)>> {
        let role = self.round_role();
        let mut retried = false;
        let mut repairs = 0_usize;
        let mut overflows = 0_usize;
        let mut usage = Usage::default();
        loop {
            let request = self.request_with(role, messages, tools);
            let prefix_hash = request.prefix_hash();
            self.freeze_catalog();
            let request_chars =
                request_chars(messages, tools.json.len(), self.provider.keeps_reasoning());
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
                    let retryable = !retried && failure.is_some_and(retryable_failure);
                    let kind = failure.map(|failure| failure.kind);
                    self.record_failure(
                        durable_attempt,
                        &error,
                        started,
                        retryable,
                        messages,
                        events,
                        emit,
                    )?;
                    if self.round_abort().is_some() {
                        return Ok(None);
                    }
                    let text = error.to_string();
                    if kind == Some(ProviderFailureKind::MalformedToolCall) {
                        if repairs >= REPAIR_LIMIT {
                            return Err(error);
                        }
                        repairs += 1;
                        self.repair_call(messages, events, emit)?;
                        continue;
                    }
                    if kind == Some(ProviderFailureKind::ContextOverflow) {
                        if overflows >= REPAIR_LIMIT {
                            return Err(error);
                        }
                        overflows += 1;
                        self.recover_overflow(
                            &text,
                            messages,
                            tools.json.len(),
                            &mut usage,
                            events,
                            emit,
                        )
                        .await?;
                        continue;
                    }
                    if !retryable {
                        if self.seats.contains_key(&role) || !self.fall_back(&text, events, emit)? {
                            return Err(error);
                        }
                        retried = false;
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

    #[expect(
        clippy::too_many_arguments,
        reason = "one record path for a failed provider attempt"
    )]
    fn record_failure(
        &mut self,
        attempt: Attempt,
        error: &Error,
        started: Instant,
        retryable: bool,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        if retryable {
            self.session.partial = crate::provider::Partial::default();
        }
        let interrupted = (!self.session.partial.text.is_empty()).then(|| Message {
            role: Role::Assistant,
            ..Message::system(self.session.partial.text.clone())
        });
        self.session
            .fail_provider(
                attempt,
                &error.to_string(),
                super::elapsed_millis(started),
                super::billable(error),
            )
            .map_err(super::session_err)?;
        messages.extend(interrupted);
        self.emit_records(events, emit)?;
        self.sync_usage(emit);
        Ok(())
    }

    fn repair_call(
        &mut self,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let edits = repair_arguments(&messages[1..]);
        self.rewrite(messages, edits)?;
        let input = UserInput::from(
            "[summon control: the provider rejected the previous tool call because \
             its arguments were invalid JSON.]",
        );
        self.session.steer(&input).map_err(super::session_err)?;
        messages.push(Message::user(input));
        self.push_event(
            events,
            emit,
            Event::Note {
                text: "malformed tool-call JSON — requesting a repaired call".to_string(),
            },
        )
    }

    async fn recover_overflow(
        &mut self,
        text: &str,
        messages: &mut Vec<Message>,
        tool_bytes: usize,
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        if let Some(cap) = crate::provider::input_cap(text) {
            self.set_context_window(Some(cap));
        }
        self.auto_compact(messages, tool_bytes, usage, events, emit)
            .await?;
        let edits = force_compact(&messages[1..]);
        self.rewrite(messages, edits)?;
        self.tools.forget_reads();
        self.push_event(
            events,
            emit,
            Event::Note {
                text: "context overflow — compacted old tool results, retrying".to_string(),
            },
        )
    }

    fn fall_back(
        &mut self,
        text: &str,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<bool> {
        let Some((provider, model)) = self.fallbacks.pop_front() else {
            return Ok(false);
        };
        let from = std::mem::replace(&mut self.model, model);
        self.provider = provider;
        self.context_sample = None;
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
        Ok(true)
    }
}

#[cfg(test)]
#[path = "retry_tests.rs"]
mod tests;

fn retryable_failure(failure: &crate::error::ProviderFailure) -> bool {
    failure.partial_output
        && failure.status.is_none()
        && matches!(
            failure.kind,
            ProviderFailureKind::Transport | ProviderFailureKind::Server
        )
}
