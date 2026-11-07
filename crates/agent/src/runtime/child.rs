use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures::stream::{FuturesUnordered, StreamExt};
use serde::Deserialize;
use tokio::sync::{mpsc::UnboundedSender, watch};
use tokio::task::JoinHandle;

use super::*;
use crate::child::{Access, ChildRecord, ChildResult, ChildState};
use crate::display::ellipsize;
use crate::durable::id::SessionId;
use crate::durable::log::{self, State, Usage as Meter};
use crate::tools::{ToolOutput, tool};
use crate::worktree::{self, Tree};

const LAST_CHARS: usize = 120;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    #[serde(default)]
    assignment: String,
    #[serde(default)]
    access: Access,
    #[serde(default)]
    workspace: Option<Workspace>,
    #[serde(default)]
    background: bool,
    #[serde(default)]
    child: Option<String>,
    #[serde(default)]
    kill: bool,
    #[serde(default)]
    clear: bool,
    #[serde(default)]
    agent: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    effort: Option<String>,
    #[serde(default)]
    endpoint: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(skip)]
    call: String,
}

impl Args {
    fn endpoint(&self) -> Result<Option<crate::endpoint::Endpoint>> {
        use crate::endpoint::{self, Requirement};
        if self.model.is_some() && self.endpoint.is_none() {
            return Err(Error::Tool(
                "task model requires an explicit endpoint".to_string(),
            ));
        }
        if self
            .model
            .as_deref()
            .is_some_and(|model| model.trim().is_empty())
        {
            return Err(Error::Tool("task model must not be empty".to_string()));
        }
        self.endpoint
            .as_deref()
            .map(|name| {
                endpoint::resolve(Some(name), self.model.as_deref(), Requirement::ModelAndKey)
            })
            .transpose()
    }
}

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Workspace {
    #[default]
    Isolated,
    Shared,
}

pub(super) struct Execution {
    pub(super) output: ToolOutput,
    pub(super) usage: Usage,
    pub(super) millis: u64,
}

pub(super) struct Spawned {
    record: ChildRecord,
    path: PathBuf,
    handle: JoinHandle<Done>,
    started: Instant,
    last: String,
    reserved_tokens: u64,
    state: watch::Sender<ChildState>,
    stay: bool,
    tool: String,
}

mod batch;
mod brief;
mod control;
mod poll;
pub(super) use poll::Kill;
mod record;
mod run;

mod spawn;

use run::{Guard, Run};

pub(super) struct Done {
    record: ChildRecord,
    tree: Option<Tree>,
    paths: Vec<String>,
    usage: Usage,
    billed: u64,
    millis: u64,
    settled: bool,
}

pub(super) fn definition() -> ToolDefinition {
    tool(
        "task",
        "Delegate one assignment to a child. Omit endpoint and model to inherit the provider and model, or select a configured endpoint and optional model for this child only. model requires endpoint; never change global defaults to select a child. The child keeps its transcript. Mail to its name reaches it at its next step and never interrupts a running tool. A child tool call does not start a parent turn. The parent turn ending does not stop it. kill true does. clear true dismisses the child's UI card without stopping it or deleting its history; combine with kill true to stop and dismiss. Same tools, MCP included, except task and ask. background true returns an id and you are told when it finishes; pass child only to read its state on request. A named child runs in the background, stays available for mail, and edits this checkout. An unnamed implement child gets an isolated checkout in a Git repository; completed edits are reconciled and conflicts keep that checkout. Pass workspace isolated or shared to force one. Do not start a summonr process. research is always read-only and gets no MCP tools.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "assignment": {"type": "string"},
                "access": {"type": "string", "enum": ["implement", "research"], "default": "implement"},
                "workspace": {"type": "string", "enum": ["isolated", "shared"], "description": "Omit for a named child to edit this checkout, or for an unnamed child to get an isolated checkout. Pass isolated or shared to force one."},
                "background": {"type": "boolean"},
                "child": {"type": "string"},
                "kill": {"type": "boolean"},
                "clear": {"type": "boolean", "description": "dismiss this child's UI card without stopping it or deleting history; requires child. Combine with kill to stop and dismiss."},
                "agent": {"type": "string", "description": "name of an agent from <agents>"},
                "name": {"type": "string", "description": "session name. mail to this name is the next turn"},
                "effort": {"type": "string", "description": "child effort: minimal, low, medium, high, xhigh, or max. Omit to inherit the selected endpoint or parent."},
                "endpoint": {"type": "string", "description": "configured endpoint name for this child only. Omit to inherit the parent selection."},
                "model": {"type": "string", "description": "model on the selected endpoint, for this child only. Requires endpoint. Omit to use that endpoint's model."}
            },
            "additionalProperties": false
        }),
    )
}

impl<P: Provider> Loop<P> {
    pub(super) fn task_available(&self) -> bool {
        self.tasks
            && self.session.depth() == 0
            && self.interactive
            && self
                .selection(crate::workflow::Role::Implement)
                .0
                .supports_children()
    }

    pub(super) fn child_budget_reserved(&self) -> u64 {
        self.spawned
            .values()
            .map(|spawned| spawned.reserved_tokens)
            .fold(0, u64::saturating_add)
    }

    pub(super) fn child_budget_running(&self) -> bool {
        self.spawned
            .values()
            .any(|spawned| spawned.reserved_tokens > 0 && !spawned.state.borrow().terminal())
    }
}

pub(super) fn start_tree(access: Access, workspace: &Path, child: &str) -> String {
    match access {
        Access::Implement => lead_tree(workspace, child),
        Access::Research => String::new(),
    }
}

pub(super) fn lead_tree(workspace: &Path, child: &str) -> String {
    crate::checkpoint::Checkpoints::open(workspace, child)
        .and_then(|points| points.tree().ok())
        .unwrap_or_default()
}

pub(super) fn current_image() -> String {
    std::env::current_exe()
        .map_or_else(|_| "unknown".to_string(), |path| path.display().to_string())
}

pub(super) fn running_place(workspace: &Path) -> String {
    format!(
        "workspace: {}\npid: {}\nimage: {}",
        workspace.display(),
        std::process::id(),
        current_image()
    )
}
