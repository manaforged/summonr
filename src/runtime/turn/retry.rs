use super::*;

const MAIL_PER_ROUND: usize = 5;

#[derive(Default)]
struct RetryState {
    retries: u32,
    continued: bool,
    repairs: usize,
    overflows: usize,
    usage: Usage,
}

struct Failed {
    error: Error,
    attempt: Attempt,
    started: Instant,
    role: workflow::Role,
}

impl<P: Provider> Loop<P> {
    pub(super) fn steer_waiting(&self) -> bool {
        !self.steer.is_empty()
            || self.steer_shared.as_ref().is_some_and(|shared| {
                !shared
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .is_empty()
            })
    }

    pub(super) fn take_mail(
        &mut self,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        drain_steer(&mut self.steer, &mut self.session, messages)?;
        if let Some(shared) = self.steer_shared.clone() {
            let mut queue = shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            drain_steer(&mut queue, &mut self.session, messages)?;
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
        let mut state = RetryState::default();
        loop {
            let request = self.request_with(role, messages, tools);
            let prefix_hash = request.prefix_hash();
            let request = request.with_prefix_hash(prefix_hash.clone());
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
                    let attempt = Failed {
                        error,
                        attempt: durable_attempt,
                        started,
                        role,
                    };
                    if !self
                        .provider_failed(attempt, &mut state, messages, tools, events, emit)
                        .await?
                    {
                        return Ok(None);
                    }
                }
            }
        }
    }

    async fn provider_failed(
        &mut self,
        failed: Failed,
        state: &mut RetryState,
        messages: &mut Vec<Message>,
        tools: &super::super::mail::Advertised,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<bool> {
        let Failed {
            error,
            attempt,
            started,
            role,
        } = failed;
        let failure = error.provider_failure();
        let retryable = state.retries < crate::limits::limits().retry_limit
            && failure.is_some_and(retryable_failure);
        let wait = failure
            .map(|failure| crate::limits::limits().retry_wait(failure.retry_after, state.retries));
        let kind = failure.map(|failure| failure.kind);
        self.record_failure(attempt, &error, started, messages, events, emit)?;
        if self.round_abort().is_some() {
            return Ok(false);
        }
        let text = error.to_string();
        if kind == Some(ProviderFailureKind::MalformedToolCall) {
            if state.repairs >= crate::limits::limits().repair_limit {
                return Err(error);
            }
            state.repairs += 1;
            self.repair_call(messages, events, emit)?;
            return Ok(true);
        }
        if kind == Some(ProviderFailureKind::Empty) && !state.continued {
            state.continued = true;
            self.continue_empty(messages, events, emit)?;
            return Ok(true);
        }
        if kind == Some(ProviderFailureKind::ContextOverflow) {
            if state.overflows >= crate::limits::limits().repair_limit {
                return Err(error);
            }
            state.overflows += 1;
            if !self
                .recover_overflow(
                    &text,
                    messages,
                    tools.json.len(),
                    &mut state.usage,
                    events,
                    emit,
                )
                .await?
            {
                return Err(error);
            }
            return Ok(true);
        }
        self.retry_or_fall_back(error, retryable, role, state, events, emit)?;
        if retryable && let Some(wait) = wait {
            tokio::time::sleep(wait).await;
        }
        Ok(true)
    }

    fn retry_or_fall_back(
        &mut self,
        error: Error,
        retryable: bool,
        role: workflow::Role,
        state: &mut RetryState,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let text = error.to_string();
        if !retryable {
            if self.seats.contains_key(&role) || !self.fall_back(&text, events, emit)? {
                return Err(error);
            }
            state.retries = 0;
            return Ok(());
        }
        state.retries += 1;
        self.push_event(
            events,
            emit,
            Event::Note {
                text: format!(
                    "stream cut; retry {} of {}: {}",
                    state.retries,
                    crate::limits::limits().retry_limit,
                    text.lines().next().unwrap_or("unknown")
                ),
            },
        )
    }

    fn record_failure(
        &mut self,
        attempt: Attempt,
        error: &Error,
        started: Instant,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
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
                text: "malformed tool-call JSON; requesting a repaired call".to_string(),
            },
        )
    }

    fn continue_empty(
        &mut self,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let input = UserInput::from(
            "[summon control: the previous reply was empty. Please continue the task.]",
        );
        self.session.steer(&input).map_err(super::session_err)?;
        messages.push(Message::user(input));
        self.push_event(
            events,
            emit,
            Event::Note {
                text: "empty reply; asking the model to continue".to_string(),
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
    ) -> Result<bool> {
        if let Some(cap) = crate::provider::input_cap(text) {
            self.set_context_window(Some(cap));
        }
        if !self
            .auto_compact(messages, tool_bytes, usage, events, emit)
            .await?
        {
            let edits = force_compact(&messages[1..]);
            if edits.is_empty() {
                return Ok(false);
            }
            self.rewrite(messages, edits)?;
            self.tools.forget_reads();
        }
        self.push_event(
            events,
            emit,
            Event::Note {
                text: "context overflow; compacted the context, retrying".to_string(),
            },
        )?;
        Ok(true)
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
        let from = self.model.clone();
        self.set_provider(provider, model);
        self.push_event(
            events,
            emit,
            Event::Note {
                text: format!(
                    "{from} failed: {}; falling back to {}",
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

fn drain_steer(
    queue: &mut std::collections::VecDeque<UserInput>,
    session: &mut SessionDriver,
    messages: &mut Vec<Message>,
) -> Result<()> {
    while let Some(input) = queue.pop_front() {
        if let Err(error) = session.steer(&input) {
            queue.push_front(input);
            return Err(super::session_err(error));
        }
        messages.push(Message::user(input));
    }
    Ok(())
}

fn retryable_failure(failure: &crate::error::ProviderFailure) -> bool {
    failure.partial_output
        && failure.status.is_none()
        && matches!(
            failure.kind,
            ProviderFailureKind::Transport | ProviderFailureKind::Server
        )
}
