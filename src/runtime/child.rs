use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures::stream::{FuturesUnordered, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;

use super::*;
use crate::child::{Access, ChildRecord, ChildResult, ChildState};
use crate::durable::id::SessionId;
use crate::durable::log::{self, State, Usage as Meter};
use crate::text::ellipsize;
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
    background: bool,
    #[serde(default)]
    child: Option<String>,
    #[serde(default)]
    kill: bool,
    #[serde(default)]
    agent: Option<String>,
    #[serde(skip)]
    call: String,
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
}

impl Spawned {
    pub(super) fn abort(&self) {
        self.handle.abort();
    }
}

mod batch;
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
        "Delegate one assignment to a child on the same provider and model. Same tools except task. Several calls in one response run together and do not see each other's edits. background true returns an id; pass child to poll, kill true to stop. implement edits an isolated checkout that shares the parent build cache when this workspace is a git repository, and edits this workspace otherwise. The parent applies each finished child's edits on its own and lists the files that landed. A conflict keeps that checkout. research is read-only.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "assignment": {"type": "string"},
                "access": {"type": "string", "enum": ["implement", "research"], "default": "implement"},
                "background": {"type": "boolean"},
                "child": {"type": "string"},
                "kill": {"type": "boolean"},
                "agent": {"type": "string", "description": "name of an agent from <agents>"}
            },
            "additionalProperties": false
        }),
    )
}

impl<P: Provider> Loop<P> {
    pub(super) fn task_available(&self) -> bool {
        self.tasks
            && self.depth == 0
            && self.interactive
            && self.seat(self.round_role()).0.supports_children()
    }

    pub(super) fn child_budget_reserved(&self) -> u64 {
        self.spawned
            .values()
            .map(|spawned| spawned.reserved_tokens)
            .fold(0, u64::saturating_add)
    }
}
