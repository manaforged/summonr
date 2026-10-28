use super::*;

#[derive(Default)]
pub(super) struct RetryState {
    retries: u32,
    continued: bool,
    repairs: usize,
    overflows: usize,
    usage: Usage,
}

impl<P: Provider> Loop<P> {
    pub(super) fn steer_waiting(&self) -> bool {
        self.steer_shared.as_ref().is_some_and(|shared| {
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
        if let Some(shared) = self.steer_shared.clone() {
            let mut queue = shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            super::steer::drain_steer(&mut queue, &mut self.session, messages)?;
        }
        let (mail, overflow) = self.take_peer_mail(crate::limits::limits().mail_per_round)?;
        for mail in mail {
            self.inject(envelope(&mail), messages)?;
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
            self.inject(note, messages)?;
        }
        Ok(())
    }

    pub(super) async fn provider_failed(
        &mut self,
        error: Error,
        state: &mut RetryState,
        messages: &mut Vec<Message>,
        tools: &super::super::mail::Advertised,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<bool> {
        let failure = error.provider_failure();
        let mut wait = failure
            .map(|failure| crate::limits::limits().retry_wait(failure.retry_after, state.retries));
        let retryable = match failure {
            Some(failure) if failure.kind == ProviderFailureKind::RateLimited => {
                let asked = failure.retry_after.map(Duration::from_secs);
                if self.remaining_time().is_some() {
                    wait = asked.or(wait);
                }
                self.can_wait(wait, state.retries)
            }
            _ => {
                state.retries < crate::limits::limits().retry_limit
                    && failure.is_some_and(retryable_failure)
            }
        };
        let kind = failure.map(|failure| failure.kind);
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
        self.retry_or_fall_back(error, retryable, state, events, emit)?;
        if retryable && let Some(wait) = wait {
            tokio::time::sleep(wait).await;
        }
        Ok(true)
    }

    fn can_wait(&self, wait: Option<Duration>, retries: u32) -> bool {
        match self.remaining_time() {
            Some(left) => wait.is_some_and(|wait| wait < left),
            None => retries < crate::limits::limits().retry_limit,
        }
    }

    fn retry_or_fall_back(
        &mut self,
        error: Error,
        retryable: bool,
        state: &mut RetryState,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        if error.provider_failure().map(|failure| failure.kind)
            == Some(ProviderFailureKind::UsageLimit)
        {
            return Err(error);
        }
        let text = error.to_string();
        if !retryable {
            if self
                .selections
                .contains_key(&crate::workflow::Role::Implement)
                || !self.fall_back(&text, events, emit)?
            {
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
                    "provider error; retry {}: {}",
                    state.retries,
                    text.lines().next().unwrap_or("unknown")
                ),
            },
        )
    }

    fn repair_call(
        &mut self,
        messages: &mut Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let edits = repair_arguments(&messages[1..]);
        self.rewrite(messages, edits)?;
        self.inject(
            "[summon control: the provider rejected the previous tool call because \
             its arguments were invalid JSON.]"
                .to_string(),
            messages,
        )?;
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
        self.inject(
            "[summon control: the previous reply was empty. Please continue the task.]".to_string(),
            messages,
        )?;
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
            self.learn_role_window(crate::workflow::Role::Implement, cap);
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
        let Some((provider, endpoint)) = self.fallbacks.pop_front() else {
            return Ok(false);
        };
        let from = self.model.clone();
        self.set_provider(provider, endpoint.model.clone());
        self.apply_endpoint(&endpoint);
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

fn retryable_failure(failure: &summonr_core::error::ProviderFailure) -> bool {
    failure.partial_output
        && failure.status.is_none()
        && matches!(
            failure.kind,
            ProviderFailureKind::Transport | ProviderFailureKind::Server
        )
}
