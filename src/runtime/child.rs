use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures::stream::{FuturesUnordered, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc::UnboundedSender;
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
    agent: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    effort: Option<String>,
    #[serde(skip)]
    call: String,
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
    staying: std::sync::Arc<std::sync::atomic::AtomicBool>,
    tool: String,
}

impl Spawned {
    pub(super) fn abort(&self) {
        self.handle.abort();
    }

    pub(super) fn background(&self) -> bool {
        self.record.background
    }
}

mod batch;
mod brief;
mod poll;
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
        "Delegate one assignment to a child on the same provider and model. The child keeps its transcript. Mail to its name is its next turn and stops its running command and background jobs. A child tool call does not start a parent turn. The parent turn ending does not stop it. kill true does. Same tools except task. background true returns an id; pass child to poll. A named child edits this checkout. An unnamed implement child gets an isolated checkout in a Git repository; completed edits are reconciled and conflicts keep that checkout. Pass workspace isolated or shared to force one. Do not start a summonr process. research is always read-only.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "assignment": {"type": "string"},
                "access": {"type": "string", "enum": ["implement", "research"], "default": "implement"},
                "workspace": {"type": "string", "enum": ["isolated", "shared"], "description": "Omit for a named child to edit this checkout, or for an unnamed child to get an isolated checkout. Pass isolated or shared to force one."},
                "background": {"type": "boolean"},
                "child": {"type": "string"},
                "kill": {"type": "boolean"},
                "agent": {"type": "string", "description": "name of an agent from <agents>"},
                "name": {"type": "string", "description": "session name. mail to this name is the next turn"},
                "effort": {"type": "string", "description": "child effort: minimal, low, medium, high, xhigh, or max. Omit to inherit the parent."}
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
