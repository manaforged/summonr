use super::checkpoint::Pretask;
use super::compaction::{force_compact, repair_arguments};
use super::*;
use crate::ProviderFailureKind;
use crate::durable::driver::{Purpose, Verdict};
use crate::durable::id::{Attempt, Call};
use crate::durable::work::{Decision, Observation, Stage};
use crate::workflow;
use std::future::Future;
use std::time::Duration;

use crate::mail::{envelope, is_mail_tool};
use crate::policy::Grant;
use crate::prompt::{Harness, harness_section};
use crate::provider::StopReason;
use crate::tools::{Abort, ToolOutput, needed_grant, readonly};
use crate::{Receipt, WorkKind, WorkProgress, goal, jobs, scout, verify};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrantAnswer {
    Deny,
    Allow,
    Always,
}

impl GrantAnswer {
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Self::Deny => "n",
            Self::Allow => "y",
            Self::Always => "always",
        }
    }
}

fn joined(earlier: Turn, later: Turn) -> Turn {
    let mut events = earlier.events;
    events.extend(later.events);
    let mut usage = earlier.usage;
    usage.add(&later.usage);
    Turn {
        text: later.text,
        events,
        rounds: earlier.rounds.saturating_add(later.rounds),
        usage,
        checks: later.checks,
        aborted: later.aborted,
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
mod call;
pub(super) use call::CallSource;
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
        let limit = Duration::from_secs(self.budget().max_secs?);
        Some(limit.saturating_sub(Duration::from_secs(self.elapsed_secs())))
    }

    pub(super) async fn wait<F>(
        completion: F,
        remaining: Option<Duration>,
    ) -> std::result::Result<AssistantMessage, WaitError>
    where
        F: Future<Output = Result<AssistantMessage>> + Send,
    {
        if let Some(remaining) = remaining {
            crate::limits::within(remaining, "the reply", completion)
                .await
                .map_err(|_timeout| WaitError::Deadline(remaining))?
                .map_err(WaitError::Provider)
        } else {
            completion.await.map_err(WaitError::Provider)
        }
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
        let images = self.selection(workflow::Role::Implement).0.takes_images();
        self.tools.set_model_images(images);
        let mut input = self.expand_mcp(input, &mut events, &mut emit).await?;
        self.tools.fold_unseen_images(&mut input).await?;
        Box::pin(self.admit(&mut input, trigger == Trigger::User, &mut emit)).await?;
        self.begin_turn(input, trigger)?;
        let result = self.run_opened(&mut events, &mut emit).await;
        if result.is_err() {
            self.close_open("turn failed", &mut emit);
        }
        result
    }

    async fn run_opened(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Turn> {
        let mut earlier: Option<Turn> = None;
        let cap = crate::limits::limits().stop_hook_streak;
        let mut streak = 0;
        loop {
            let (latest, finished) = Box::pin(self.run_timed(events, emit)).await?;
            if latest.rounds > 1 {
                streak = 0;
            }
            let active = earlier.is_some();
            let turn = match earlier.take() {
                Some(earlier) => joined(earlier, latest),
                None => latest,
            };
            if !finished {
                return Ok(turn);
            }
            let Some(reason) = Box::pin(self.stop_hook(&turn, active, emit)).await else {
                return Ok(turn);
            };
            if turn.aborted {
                return Ok(turn);
            }
            if streak >= cap {
                emit(&Event::Note {
                    text: format!("stop hook: kept the turn going {cap} times in a row; ending it"),
                });
                return Ok(turn);
            }
            streak += 1;
            let text = format!("Harness: a stop hook kept the turn going: {reason}");
            self.begin_turn(UserInput::from(text), Trigger::Mail)?;
            earlier = Some(turn);
        }
    }

    async fn run_timed(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<(Turn, bool)> {
        let progress = self.work_progress()?;
        self.push_event(events, emit, Event::Work { progress })?;
        self.measure_baseline(events, emit).await?;
        let mut pre = self.checkpoint();
        let mut usage = Usage::default();
        let mut rounds = 0;
        let remaining = self.remaining_time();
        let turn = self.run_turn(&mut pre, &mut *emit, events, &mut usage, &mut rounds);
        let result = match remaining {
            Some(remaining) => crate::limits::within(remaining, "the turn", turn).await,
            None => Ok(turn.await),
        };
        if result.is_err()
            && let Some(hub) = &self.mcp
        {
            hub.cancel_inflight().await;
        }
        self.sync_usage(emit);
        match result {
            Ok(result) => Ok((result?, true)),
            Err(_timeout) => {
                let turn = self
                    .abort_turn(
                        Abort::time_limit(),
                        events,
                        emit,
                        &mut usage,
                        rounds,
                        &mut pre,
                    )
                    .await?;
                Ok((turn, false))
            }
        }
    }

    pub(crate) async fn turn_mail_with(&mut self, emit: impl FnMut(&Event)) -> Result<Turn> {
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
