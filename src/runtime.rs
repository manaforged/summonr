use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::durable::driver::SessionDriver;
use crate::durable::effect::Usage as Meter;
use crate::error::{Error, Result};
use crate::policy::Policy;
use crate::provider::{
    AssistantMessage, CompletionRequest, Message, Provider, Role, ToolCall, ToolDefinition,
    ToolFunction, ToolKind, Usage, UserInput,
};
use crate::tools::{Event, ToolRegistry};
use crate::verify::Outcome;

const DEFAULT_MAX_ROUNDS: usize = 64;
pub(crate) const DEFAULT_ROUND_OUTPUT_TOKENS: u32 = 8_192;
/// Output ceiling for auxiliary calls (the goal judge).
pub(crate) const AUX_OUTPUT_TOKENS: u32 = 512;

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
                "budget: {elapsed_secs}s reached the {max}s wall-clock limit"
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
    system_prompt: String,
    plan: bool,
    tools: ToolRegistry,
    /// Advertised tool JSON, filled once before the first provider call.
    schema: Option<Vec<ToolDefinition>>,
    task_enabled: bool,
    task: Option<crate::task::TaskState>,
    history: Vec<Message>,
    session: SessionDriver,
    max_rounds: usize,
    verify: Option<Vec<String>>,
    budget: Budget,
    spent_tokens: u64,
    /// Last live request size and provider-reported prompt tokens.
    context_sample: Option<(usize, u32)>,
    started: Instant,
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

mod bus;
mod compaction;
mod configure;
mod nested;
mod turn;

pub(super) fn session_err(error: impl std::fmt::Display) -> Error {
    Error::Config(error.to_string())
}

pub(super) fn meter(usage: &Usage) -> Meter {
    Meter {
        input_tokens: Some(u64::from(usage.prompt_tokens)),
        output_tokens: Some(u64::from(usage.completion_tokens)),
        cached_input_tokens: Some(u64::from(usage.cached_tokens)),
        live_millis: None,
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
