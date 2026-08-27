use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::durable::driver::{Exposure, SessionDriver};
use crate::durable::log::Usage as Meter;
use crate::error::{Error, Result};
use crate::policy::Policy;
use crate::provider::{
    AssistantMessage, CompletionRequest, Message, Provider, Role, ToolCall, ToolDefinition,
    ToolFunction, ToolKind, Usage, UserInput,
};
use crate::tools::{Event, ToolRegistry};
use crate::verify::Outcome;
use crate::workflow;

const DEFAULT_MAX_ROUNDS: usize = 64;
pub(crate) const DEFAULT_ROUND_OUTPUT_TOKENS: u32 = 8_192;
/// Output ceiling for the Goal completion judge.
pub(crate) const JUDGE_OUTPUT_TOKENS: u32 = 512;

/// Session ceilings. Breach aborts the turn, persists it, and exits nonzero.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Budget {
    pub max_tokens: Option<u64>,
    pub max_secs: Option<u64>,
}

impl Budget {
    /// No ceilings.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    fn breach(&self, spent_tokens: u64, elapsed_secs: u64) -> Option<String> {
        if let Some(max) = self.max_secs
            && elapsed_secs >= max
        {
            return Some(format!(
                "budget: {elapsed_secs}s reached the {max}s work-time limit"
            ));
        }
        if let Some(max) = self.max_tokens
            && spent_tokens >= max
        {
            return Some(format!(
                "budget: {spent_tokens} tokens reached the {max} token limit"
            ));
        }
        None
    }
}

/// The result of one user turn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Turn {
    pub text: String,
    pub events: Vec<Event>,
    pub rounds: usize,
    pub usage: Usage,
    /// Verify verdict when a check ran this turn.
    pub verify: Option<Outcome>,
    /// True when the turn ended on a budget or round abort.
    pub aborted: bool,
}

/// A stateful conversation loop with a durable session log.
pub struct Loop<P> {
    provider: P,
    model: String,
    /// Roles with their own seat; the rest run on `provider` + `model`.
    seats: BTreeMap<workflow::Role, (P, String)>,
    system_prompt: String,
    plan: bool,
    tools: ToolRegistry,
    /// Advertised tool JSON, filled once before the first provider call.
    schema: Option<Vec<ToolDefinition>>,
    history: Vec<Message>,
    session: SessionDriver,
    max_rounds: usize,
    verify: Option<Vec<String>>,
    budget: Budget,
    spent_tokens: u64,
    /// Last live request size and provider-reported prompt tokens.
    context_sample: Option<(usize, u32)>,
    /// The model's context window in tokens, when the endpoint says.
    context_window: Option<u32>,
    workspace: PathBuf,
    bus: Option<crate::bus::Bus>,
    /// Mail observed by the UI but not yet injected into a provider round.
    mail: std::collections::VecDeque<crate::bus::Mail>,
    /// User steering: text injected before the next provider round.
    steer: std::collections::VecDeque<UserInput>,
    /// The worker's live steering source, shared while a turn runs.
    steer_shared: Option<std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<UserInput>>>>,
}

mod aside;
mod bus;
mod compaction;
mod configure;
mod turn;

impl<P: Provider> Loop<P> {
    /// The seat a role runs on: its own, or the driving seat.
    fn seat(&self, role: workflow::Role) -> (&P, &str) {
        self.seats.get(&role).map_or(
            (&self.provider, self.model.as_str()),
            |(provider, model)| (provider, model.as_str()),
        )
    }

    /// The role an agent round belongs to.
    fn round_role(&self) -> workflow::Role {
        if self.plan {
            workflow::Role::Plan
        } else {
            workflow::Role::Implement
        }
    }
}

pub(super) fn session_err(error: impl std::fmt::Display) -> Error {
    Error::Config(error.to_string())
}

pub(super) fn meter(usage: &Usage, millis: u64) -> Meter {
    Meter {
        input: u64::from(usage.prompt_tokens),
        output: u64::from(usage.completion_tokens),
        cached: u64::from(usage.cached_tokens),
        millis,
    }
}

pub(super) fn exposure(request: &CompletionRequest, bytes: &[u8]) -> Exposure {
    Exposure {
        // Estimate input tokens from request bytes (~4 bytes per token) so a
        // failed round is charged near its real cost, not ~4x it, which would
        // abort budgets early.
        max_input_tokens: u64::try_from(bytes.len() / 4).unwrap_or(u64::MAX),
        max_output_tokens: u64::from(request.max_output_tokens),
    }
}

pub(super) fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
