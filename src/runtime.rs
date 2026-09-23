use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::bus::{Bus, Mail};
use crate::durable::driver::{Exposure, SessionDriver};
use crate::durable::log::Usage as Meter;
use crate::error::{Error, Result};
use crate::herdr::Reporter;
use crate::policy::{Grant, Policy};
use crate::provider::{
    AssistantMessage, CompletionRequest, Effort, Message, Price, Provider, Role, ToolCall,
    ToolDefinition, ToolFunction, ToolKind, Usage, UserInput,
};
use crate::tools::{Event, ToolRegistry};
use crate::verify::Outcome;
use crate::workflow;

#[cfg(not(test))]
pub(crate) const JOB_WAIT_SECS: u64 = 120;
#[cfg(test)]
pub(crate) const JOB_WAIT_SECS: u64 = 1;
pub(crate) const JOB_WAIT: std::time::Duration = std::time::Duration::from_secs(JOB_WAIT_SECS);
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Budget {
    pub max_tokens: Option<u64>,
    pub max_secs: Option<u64>,
}

impl Budget {
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RunMode {
    #[default]
    Auto,
    Plan,
    Work,
    Goal,
}

impl RunMode {
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::Plan,
            Self::Plan => Self::Auto,
            Self::Work => Self::Auto,
            Self::Goal => Self::Auto,
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Plan => "plan",
            Self::Work => "work",
            Self::Goal => "goal",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub text: String,
    pub events: Vec<Event>,
    pub rounds: usize,
    pub usage: Usage,
    pub checks: Vec<Outcome>,
    pub aborted: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepRun {
    Done(i32),
    Run {
        index: usize,
        total: usize,
        prompt: String,
    },
}

pub struct Loop<P> {
    provider: P,
    model: String,
    seats: BTreeMap<workflow::Role, (P, String)>,
    tuning: BTreeMap<workflow::Role, (Option<Effort>, Option<Price>)>,
    price: Option<Price>,
    system_prompt: String,
    mode: RunMode,
    plan_first: bool,
    tools: ToolRegistry,
    schema: Option<Vec<ToolDefinition>>,

    herdr: Reporter,
    session: SessionDriver,
    verify: Option<Vec<String>>,
    budget: Budget,
    spent_tokens: u64,
    context_sample: Option<(usize, u32)>,
    context_window: Option<u32>,
    probe: Option<tokio::task::JoinHandle<Option<u32>>>,
    effort: Option<Effort>,
    turn_started: u64,
    accepted: bool,
    summary_failed: bool,
    workspace: PathBuf,
    bus: Option<Bus>,
    mcp: Option<Arc<crate::mcp::Hub>>,
    guard: guard::Guard,
    checkpoints: Option<crate::checkpoint::Checkpoints>,
    fallbacks: VecDeque<(P, String)>,
    hooks: crate::hooks::Hooks,
    compact_tokens: Option<u32>,
    mcp_start: Option<tokio::task::JoinHandle<crate::mcp::Hub>>,
    mail: VecDeque<Mail>,
    steer: VecDeque<UserInput>,
    steer_shared: Option<Arc<Mutex<VecDeque<UserInput>>>>,
    depth: u8,
    tasks: bool,
    manual: bool,
    asked: Vec<Grant>,
    blocks: (usize, usize),
    spawned: BTreeMap<String, child::Spawned>,
    mailbox: (UnboundedSender<Event>, UnboundedReceiver<Event>),
    catalog_frozen: bool,
}

mod approval;
mod aside;
mod bus;
mod checkpoint;
mod child;
mod compaction;
mod configure;
mod guard;
mod hooks;
mod mcp;
mod turn;

impl<P: Provider> Loop<P> {
    pub async fn shell(&self, command: &str) -> Result<(String, bool)> {
        let call = ToolCall {
            continuation: None,
            id: format!("shell-{}", self.session_id()),
            kind: ToolKind::Function,
            function: ToolFunction {
                name: "run_command".to_string(),
                arguments: serde_json::json!({ "command": command }).to_string(),
            },
        };
        let output = self.tools.execute_outcome(&call).await?;
        Ok((output.text, output.success))
    }

    fn seat(&self, role: workflow::Role) -> (&P, &str) {
        self.seats.get(&role).map_or(
            (&self.provider, self.model.as_str()),
            |(provider, model)| (provider, model.as_str()),
        )
    }

    pub(crate) fn effort_for(&self, role: workflow::Role) -> Option<Effort> {
        let effort = self.configured_effort(role);
        if self.boosted(role) {
            return Some(Effort::Medium);
        }
        effort
    }

    fn configured_effort(&self, role: workflow::Role) -> Option<Effort> {
        self.tuning
            .get(&role)
            .and_then(|(effort, _)| *effort)
            .or(self.effort)
    }

    pub(crate) fn boosted(&self, role: workflow::Role) -> bool {
        self.guard.strained()
            && role != workflow::Role::Summary
            && self.configured_effort(role) == Some(Effort::Low)
    }

    pub(crate) fn price_for(&self, role: workflow::Role) -> Option<Price> {
        self.tuning
            .get(&role)
            .and_then(|(_, price)| *price)
            .or(self.price)
    }

    pub(crate) fn request<'a>(
        &self,
        role: workflow::Role,
        messages: &'a [Message],
        tools: &'a [ToolDefinition],
        emit_deltas: bool,
    ) -> CompletionRequest<'a> {
        CompletionRequest::new(
            self.session_id(),
            self.seat(role).1.to_string(),
            messages,
            tools,
            None,
            emit_deltas,
            self.effort_for(role),
        )
    }

    fn round_role(&self) -> workflow::Role {
        if self.mode == RunMode::Plan {
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

pub(crate) fn billable(error: &Error) -> bool {
    error
        .provider_failure()
        .is_some_and(|failure| failure.partial_output)
}

pub(super) fn exposure(request: &CompletionRequest<'_>, request_chars: usize) -> Exposure {
    Exposure {
        max_input_tokens: u64::try_from(request_chars / 4).unwrap_or(u64::MAX),
        max_output_tokens: request.max_output_tokens.map_or(0, u64::from),
    }
}

pub(super) fn elapsed_millis(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}
