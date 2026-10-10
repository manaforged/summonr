use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::durable::driver::{Exposure, SessionDriver};
use crate::durable::log::{Usage as Meter, UsageMeta};
use crate::error::{Error, Result};
use crate::herdr::Reporter;
use crate::mail::{Mail, Mailbox};
use crate::policy::Policy;
use crate::provider::{
    AssistantMessage, CompletionRequest, Effort, Message, Price, Provider, Role, ToolCall,
    ToolDefinition, ToolFunction, ToolKind, Usage, UserInput,
};
use crate::tools::{Event, ToolRegistry};
use crate::verify::Outcome;
use crate::workflow;

pub(crate) use turn::{GrantAnswer, Steering};
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

    fn breach(&self, spent_tokens: u64, elapsed_secs: u64) -> Option<crate::tools::Abort> {
        if let Some(max) = self.max_secs
            && elapsed_secs >= max
        {
            return Some(crate::tools::Abort {
                code: crate::tools::AbortCode::TimeBudget,
                reason: format!("budget: {elapsed_secs}s reached the {max}s work-time limit"),
            });
        }
        if let Some(max) = self.max_tokens
            && spent_tokens >= max
        {
            return Some(crate::tools::Abort {
                code: crate::tools::AbortCode::TokenBudget,
                reason: format!("budget: {spent_tokens} tokens reached the {max} token limit"),
            });
        }
        None
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RunMode {
    #[default]
    Auto,
    Work,
    Goal,
}

impl RunMode {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
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
    tools: ToolRegistry,
    schema: Option<Arc<mail::Advertised>>,

    herdr: Reporter,
    session: SessionDriver,
    verify: Option<Vec<String>>,
    check_file: Option<Vec<u8>>,
    budget: Budget,
    spent_tokens: u64,
    run_base: (u64, u64),
    context_sample: Option<(usize, u32)>,
    context_window: Option<u32>,
    probe: Option<tokio::task::JoinHandle<Option<u32>>>,
    effort: Option<Effort>,
    turn_started: u64,
    shield_epoch: u64,
    summary_wait: usize,
    workspace: PathBuf,
    mailbox: Option<Mailbox>,
    mail_module: bool,
    mcp: Option<Arc<crate::mcp::Hub>>,
    checkpoints: Option<crate::checkpoint::Checkpoints>,
    fallbacks: VecDeque<(P, String)>,
    hooks: crate::hooks::Hooks,
    compact_tokens: Option<u32>,
    mcp_wanted: bool,
    mcp_board: crate::mcp::Board,
    inbox: VecDeque<Mail>,
    steer: VecDeque<UserInput>,
    steer_shared: Option<Arc<turn::Steering>>,
    depth: u8,
    tasks: bool,
    interactive: bool,
    spawned: BTreeMap<String, child::Spawned>,
    events: (UnboundedSender<Event>, UnboundedReceiver<Event>),
    catalog_frozen: bool,
}

mod aside;
mod checkpoint;
mod child;
mod compaction;
mod configure;
mod hooks;
mod mail;
mod mcp;
mod turn;

impl<P: Provider> Loop<P> {
    pub async fn shell(&self, command: &str) -> Result<(String, bool)> {
        let call = ToolCall {
            continuation: None,
            id: format!("shell-{}", self.session_id()),
            kind: ToolKind::Function,
            function: ToolFunction {
                checked: Default::default(),
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
        self.configured_effort(role)
    }

    fn configured_effort(&self, role: workflow::Role) -> Option<Effort> {
        self.tuning
            .get(&role)
            .and_then(|(effort, _)| *effort)
            .or(self.effort)
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

    pub(crate) fn request_with<'a>(
        &self,
        role: workflow::Role,
        messages: &'a [Message],
        advertised: &'a mail::Advertised,
    ) -> CompletionRequest<'a> {
        let sent = messages
            .split_last()
            .filter(|(last, _)| last.role == Role::Assistant && last.tool_calls.is_empty())
            .map_or(messages, |(_, earlier)| earlier);
        self.request(role, sent, &advertised.tools, true)
            .with_tools_json(&advertised.json)
    }

    fn round_role(&self) -> workflow::Role {
        workflow::Role::Implement
    }
}

pub(super) fn session_err(error: impl std::fmt::Display) -> Error {
    Error::Config(error.to_string())
}

pub(super) fn meter(usage: &Usage, millis: u64) -> Meter {
    Meter {
        input: u64::from(usage.prompt_tokens),
        output: u64::from(usage.completion_tokens),
        reasoning: u64::from(usage.reasoning_tokens),
        cached: u64::from(usage.cached_tokens),
        written: u64::from(usage.cache_write_tokens),
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
