use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::durable::id::SessionId;
use crate::provider::Usage;

pub(crate) const ASSIGNMENT_MAX_CHARS: usize = 4_000;
pub(crate) const RESULT_MAX_CHARS: usize = 4_000;
pub(crate) const PATHS_MAX: usize = 100;
pub(crate) const PATH_MAX_CHARS: usize = 512;
pub(crate) const MAX_ROUNDS: usize = 4;
pub(crate) const MAX_TOKENS: u64 = 32_768;
pub(crate) const MAX_SECS: u64 = 300;
pub(crate) const MAX_PARALLEL: usize = 4;
pub(crate) const MAX_PER_PARENT: usize = 64;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Access {
    #[default]
    Implement,
    Research,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ChildRecord {
    pub(crate) version: u8,
    pub(crate) id: SessionId,
    pub(crate) parent: SessionId,
    pub(crate) assignment: String,
    pub(crate) model: String,
    pub(crate) workspace: PathBuf,
    pub(crate) depth: u8,
    #[serde(default)]
    pub(crate) access: Access,
    pub(crate) state: ChildState,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "status", content = "result", rename_all = "snake_case")]
pub(crate) enum ChildState {
    Running,
    Completed(ChildResult),
    Aborted(ChildResult),
    Failed(ChildResult),
    Interrupted(ChildResult),
}

impl ChildState {
    pub(crate) fn terminal(&self) -> bool {
        !matches!(self, Self::Running)
    }
}

impl ChildRecord {
    pub(crate) fn interrupt(&self, reason: &str) -> Self {
        let mut record = self.clone();
        record.state = ChildState::Interrupted(ChildResult {
            text: String::new(),
            changed_paths: Vec::new(),
            changed_paths_total: 0,
            rounds: 0,
            usage: Usage::default(),
            billed_tokens: 0,
            elapsed_millis: 0,
            diagnostic: Some(bounded(reason, RESULT_MAX_CHARS)),
        });
        record
    }

    pub(crate) fn same_child(&self, other: &Self) -> bool {
        self.version == other.version
            && self.id == other.id
            && self.parent == other.parent
            && self.assignment == other.assignment
            && self.model == other.model
            && self.workspace == other.workspace
            && self.depth == other.depth
            && self.access == other.access
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ChildResult {
    pub(crate) text: String,
    pub(crate) changed_paths: Vec<String>,
    pub(crate) changed_paths_total: usize,
    pub(crate) rounds: usize,
    pub(crate) usage: Usage,
    pub(crate) billed_tokens: u64,
    pub(crate) elapsed_millis: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) diagnostic: Option<String>,
}

pub(crate) fn bounded(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    let clipped: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{clipped}…")
    } else {
        clipped
    }
}
