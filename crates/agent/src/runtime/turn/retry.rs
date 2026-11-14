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
    pub(super) fn interrupted(&self) -> bool {
        self.input_interrupts() && self.user_input_pending()
    }

    pub(super) fn steer_waiting(&self) -> bool {
        !self.inbox.is_empty()
            || self
                .mailbox
                .as_ref()
                .is_some_and(|mailbox| mailbox.pending().unwrap_or(true))
            || self.user_input_pending()
    }

    fn user_input_pending(&self) -> bool {
        self.steer_shared.as_ref().is_some_and(|shared| {
            !shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty()
        })
    }

    pub(super) fn take_mail(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        if let Some(shared) = self.steer_shared.clone() {
            let mut queue = shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let delivered = super::steer::drain_steer(&mut queue, &mut self.session);
            drop(queue);
            for text in delivered? {
                self.push_event(events, emit, Event::Steered { text })?;
            }
        }
        let (mail, overflow) = self.take_peer_mail(crate::limits::limits().mail_per_round)?;
        for mail in mail {
            self.inject(envelope(&mail))?;
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
            self.inject(note)?;
        }
        Ok(())
    }

    pub(super) async fn provider_failed(
        &mut self,
        error: Error,
        state: &mut RetryState,
        tools: &super::super::mail::Advertised,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<bool> {
        let failure = error.provider_failure();
        let (retryable, wait) = self.retry_wait_budget(failure, state.retries);
        if self.round_abort().is_some() {
            return Ok(false);
        }
        let Some(error) = self
            .recover_provider_kind(
                failure.map(|item| item.kind),
                error,
                state,
                tools.json.len(),
                events,
                emit,
            )
            .await?
        else {
            return Ok(true);
        };
        self.retry_or_fall_back(error, retryable, state, events, emit)?;
        if retryable && let Some(wait) = wait {
            tokio::time::sleep(wait).await;
        }
        Ok(true)
    }

    fn retry_wait_budget(
        &self,
        failure: Option<&summonr_core::error::ProviderFailure>,
        retries: u32,
    ) -> (bool, Option<Duration>) {
        let mut wait =
            failure.map(|item| crate::limits::limits().retry_wait(item.retry_after, retries));
        let retryable = if let Some(item) = failure
            && item.kind == ProviderFailureKind::RateLimited
        {
            if self.remaining_time().is_some() {
                wait = Some(crate::limits::limits().budget_wait(item.retry_after, retries));
            }
            self.can_wait(wait, retries)
        } else {
            retries < crate::limits::limits().retry_limit
                && failure.is_some_and(retryable_failure)
        };
        (retryable, wait)
    }

    async fn recover_provider_kind(
        &mut self,
        kind: Option<ProviderFailureKind>,
        error: Error,
        state: &mut RetryState,
        tool_bytes: usize,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Option<Error>> {
        match kind {
            Some(ProviderFailureKind::MalformedToolCall) => {
                if state.repairs >= crate::limits::limits().repair_limit {
                    return Err(error);
                }
                state.repairs += 1;
                self.repair_call(events, emit)?;
                Ok(None)
            }
            Some(ProviderFailureKind::Empty) if !state.continued => {
                state.continued = true;
                self.continue_empty(events, emit)?;
                Ok(None)
            }
            Some(ProviderFailureKind::ContextOverflow) => {
                if state.overflows >= crate::limits::limits().repair_limit {
                    return Err(error);
                }
                state.overflows += 1;
                let text = error.to_string();
                if !self
                    .recover_overflow(&text, tool_bytes, &mut state.usage, events, emit)
                    .await?
                {
                    return Err(error);
                }
                Ok(None)
            }
            Some(_) | None => Ok(Some(error)),
        }
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
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        let edits = repair_arguments(self.session.history());
        self.rewrite(edits)?;
        self.inject(
            "[summon control: the provider rejected the previous tool call because \
             its arguments were invalid JSON.]"
                .to_string(),
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
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        self.inject(
            "[summon control: the previous reply was empty. Please continue the task.]".to_string(),
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
        tool_bytes: usize,
        usage: &mut Usage,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<bool> {
        if let Some(cap) = crate::provider::input_cap(text) {
            self.learn_role_window(crate::workflow::Role::Implement, cap);
        }
        if !self.auto_compact(tool_bytes, usage, events, emit).await? {
            let edits = force_compact(self.session.history());
            if edits.is_empty() {
                return Ok(false);
            }
            self.rewrite(edits)?;
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
