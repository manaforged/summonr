use super::checkpoint::Pretask;
use super::compaction::{force_compact, repair_arguments};
use super::*;
use crate::ProviderFailureKind;
use crate::durable::driver::{Purpose, Verdict};
use crate::durable::id::{Attempt, Call};
use crate::durable::work::{Decision, Observation, Stage};
use crate::hooks::HookEvent;
use crate::workflow;
use std::future::Future;
use std::time::Duration;

use crate::mail::{envelope, is_mail_tool};
use crate::policy::Grant;
use crate::prompt::{Harness, harness_section};
use crate::provider::StopReason;
use crate::session::decorate;
use crate::tools::{Abort, ToolOutput, needed_grant, readonly};
use crate::{Receipt, WorkKind, WorkProgress, goal, jobs, scout, verify};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GrantAnswer {
    Deny,
    Allow,
    Always,
}

impl GrantAnswer {
    #[must_use]
    pub(crate) fn token(self) -> &'static str {
        match self {
            Self::Deny => "n",
            Self::Allow => "y",
            Self::Always => "always",
        }
    }
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RetryPolicy {
    None,
    Round,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Trigger {
    User,
    Mail,
}

#[derive(Clone, Copy)]
pub(super) struct CallOptions {
    pub(super) stream: bool,
    pub(super) retry: RetryPolicy,
}

pub(super) struct Reply {
    pub(super) message: AssistantMessage,
    pub(super) chars: usize,
    pub(super) open: Option<(Attempt, Meter, UsageMeta)>,
}

pub(super) struct Missed {
    pub(super) error: WaitError,
    pub(super) interrupted: Option<Message>,
}

pub(super) enum WaitError {
    Deadline(Duration),
    Provider(Error),
    Steered,
}

impl WaitError {
    pub(super) fn millis(&self, started: Instant) -> u64 {
        let elapsed = super::elapsed_millis(started);
        match self {
            Self::Deadline(remaining) => {
                elapsed.max(u64::try_from(remaining.as_millis()).unwrap_or(u64::MAX))
            }
            Self::Provider(_) | Self::Steered => elapsed,
        }
    }

    pub(super) fn error(self) -> Error {
        match self {
            Self::Deadline(_) => Error::Config(Abort::time_limit().reason),
            Self::Provider(error) => error,
            Self::Steered => Error::Config("interrupted by an incoming message".to_string()),
        }
    }
}

mod ask;
pub(super) use ask::definition as ask_definition;
mod calls;
mod checks;
mod close;
mod contract;
mod elicit;
pub(crate) use elicit::Steering;
mod finish;
mod grant;
mod metric;
mod repeat;
mod retry;
mod round;
mod steer;
mod usage;

#[cfg(test)]
mod finish_tests;
#[cfg(test)]
mod permission_tests;
#[cfg(test)]
mod repeat_tests;

impl<P: Provider> Loop<P> {
    pub(super) fn remaining_time(&self) -> Option<Duration> {
        let limit = crate::limits::seconds(self.budget().max_secs?);
        Some(limit.saturating_sub(crate::limits::seconds(self.elapsed_secs())))
    }

    pub(super) async fn call(
        &mut self,
        role: workflow::Role,
        request: CompletionRequest<'_>,
        options: CallOptions,
    ) -> Result<std::result::Result<Reply, Missed>> {
        if options.retry == RetryPolicy::None
            && let Some(reason) = self.round_abort()
        {
            return Err(Error::Config(reason));
        }
        let prefix_hash = request.prefix_hash();
        let request = request.with_prefix_hash(prefix_hash.clone());
        let tool_bytes = request.tools_json.map_or_else(
            || crate::provider::ToolsJson::encode(request.tools).len(),
            crate::provider::ToolsJson::len,
        );
        let chars = request_chars(
            request.messages,
            tool_bytes,
            self.selection(role).0.keeps_reasoning(),
        );
        let attempt = if self.session.turn_open() {
            let exposure = super::exposure(&request, chars);
            let purpose = purpose_of(role);
            Some(
                self.session
                    .start_round(purpose, &request.model, exposure)?,
            )
        } else {
            None
        };
        let started = Instant::now();
        let remaining = self.remaining_time();
        let input = self.reply_input_wait();
        let (provider, _) = super::selected(&self.selections, &self.provider, &self.model, role);
        let completion = if options.stream {
            provider.stream(&request, &mut self.session.partial)
        } else {
            provider.complete(&request)
        };
        let waited = tokio::select! {
            biased;
            result = Self::wait(completion, remaining) => result,
            result = input, if options.retry == RetryPolicy::Round => {
                Err(result.err().map_or(WaitError::Steered, WaitError::Provider))
            }
        };
        let message = match waited {
            Ok(message) => message,
            Err(error) => {
                let text = &self.session.partial.text;
                let interrupted = (!text.is_empty()).then(|| Message {
                    role: Role::Assistant,
                    ..Message::system(text.clone())
                });
                if let Some(attempt) = attempt {
                    let millis = error.millis(started);
                    let (diagnostic, billable) = match &error {
                        WaitError::Deadline(_) => (Abort::time_limit().reason, true),
                        WaitError::Steered => {
                            ("interrupted by an incoming message".to_string(), true)
                        }
                        WaitError::Provider(error) => (error.to_string(), super::billable(error)),
                    };
                    self.session
                        .fail_provider(attempt, &diagnostic, millis, billable)?;
                }
                return Ok(Err(Missed { error, interrupted }));
            }
        };
        self.session.partial = Default::default();
        let meter = Meter::measured(&message.usage, super::elapsed_millis(started));
        let meta = UsageMeta {
            continuation: message.continuation.clone(),
            prefix_hash,
            cost: self
                .price_for(role)
                .map(|price| price.cost(meter.input, meter.output, meter.cached, meter.written)),
        };
        let open = match (attempt, options.retry) {
            (Some(attempt), RetryPolicy::Round) => Some((attempt, meter, meta)),
            (Some(attempt), RetryPolicy::None) => {
                self.session.finish_provider(
                    attempt,
                    message.content.as_deref().unwrap_or(""),
                    message.reasoning_content.as_deref().unwrap_or(""),
                    meter,
                    meta,
                )?;
                None
            }
            (None, _) => {
                self.session
                    .charge(meter, "outside a turn", &request.model, true, meta)?;
                None
            }
        };
        Ok(Ok(Reply {
            message,
            chars,
            open,
        }))
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

    pub(super) fn estimated_prompt_tokens(&self, message_chars: usize, tool_bytes: usize) -> usize {
        super::compaction::estimate(message_chars + tool_bytes, self.context_sample)
    }

    pub async fn turn(&mut self, prompt: &str) -> Result<Turn> {
        self.turn_with(prompt, |_| {}).await
    }

    pub async fn turn_with(
        &mut self,
        input: impl Into<UserInput>,
        emit: impl FnMut(&Event),
    ) -> Result<Turn> {
        self.run_input(input.into(), Trigger::User, emit).await
    }

    async fn run_input(
        &mut self,
        input: UserInput,
        trigger: Trigger,
        mut emit: impl FnMut(&Event),
    ) -> Result<Turn> {
        let _report = self.herdr.turn();
        self.close_open("turn interrupted", &mut emit);
        let mut events = Vec::new();
        let mut input = self.expand_mcp(input, &mut events, &mut emit).await?;
        self.tools.fold_unseen_images(&mut input).await?;
        let messages = self.begin_turn(input, trigger)?;
        let result = self.run_opened(messages, &mut events, &mut emit).await;
        if result.is_err() {
            self.close_open("turn failed", &mut emit);
        }
        result
    }

    async fn run_opened(
        &mut self,
        messages: Vec<Message>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Turn> {
        let progress = self.work_progress()?;
        self.push_event(events, emit, Event::Work { progress })?;
        self.measure_baseline(events, emit).await?;
        let mut pre = self.checkpoint();
        let mut usage = Usage::default();
        let mut rounds = 0;
        let remaining = self.remaining_time();
        let turn = self.run_turn(
            messages,
            &mut pre,
            &mut *emit,
            events,
            &mut usage,
            &mut rounds,
        );
        let result = match remaining {
            Some(remaining) => tokio::time::timeout(remaining, turn)
                .await
                .map_err(|_elapsed| ()),
            None => Ok(turn.await),
        };
        if result.is_err()
            && let Some(hub) = &self.mcp
        {
            hub.cancel_inflight().await;
        }
        self.sync_usage(emit);
        if let Ok(Ok(turn)) = &result {
            let payload = serde_json::json!({
                "event": "stop",
                "text": turn.text,
                "aborted": turn.aborted,
            });
            if let Some(note) = self.hook(HookEvent::Stop, None, payload).await.note() {
                emit(&Event::Note {
                    text: format!("stop hook: {note}"),
                });
            }
        }
        match result {
            Ok(result) => result,
            Err(()) => {
                self.abort_turn(
                    Abort::time_limit(),
                    events,
                    emit,
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
        let input = UserInput::from("[summon: peer mail is pending in this Work]");
        self.run_input(input, Trigger::Mail, emit).await
    }

    pub(super) fn sync_usage(&mut self, emit: &mut impl FnMut(&Event)) {
        emit(&Event::Unreported {
            tokens: self.session.usage().1,
        });
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
        workflow::Role::Scout => Purpose::Scout,
        workflow::Role::Title => Purpose::Title,
    }
}

fn request_chars(messages: &[Message], tool_bytes: usize, reasoning_on_wire: bool) -> usize {
    messages
        .iter()
        .map(|message| super::compaction::message_chars(message, reasoning_on_wire))
        .sum::<usize>()
        + tool_bytes
}
