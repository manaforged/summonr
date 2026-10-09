use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::durable::id::SessionId;
use crate::provider::Usage;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    #[default]
    Implement,
    Research,
}

impl Access {
    pub fn name(self) -> &'static str {
        match self {
            Self::Implement => "implement",
            Self::Research => "research",
        }
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) call: Option<String>,
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
            diagnostic: Some(reason.to_string()),
            kept: None,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) kept: Option<crate::tools::Kept>,
}

#[must_use]
pub fn tag(id: &str) -> String {
    let chars: Vec<char> = id.chars().collect();
    chars[chars.len().saturating_sub(8)..].iter().collect()
}
