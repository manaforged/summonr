use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::durable::id::SessionId;
use crate::provider::Usage;

pub use summonr_types::Access;
pub(crate) use summonr_types::ChildScope;

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ChildRecord {
    pub version: u8,
    pub id: SessionId,
    pub parent: SessionId,
    pub assignment: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub agent: String,
    pub workspace: PathBuf,
    pub depth: u8,
    #[serde(default)]
    pub access: Access,
    #[serde(default)]
    pub background: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base: String,
    pub state: ChildState,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "status", content = "result", rename_all = "snake_case")]
pub enum ChildState {
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

    /// Returns the terminal child result when the child is no longer running.
    #[must_use]
    pub fn result(&self) -> Option<&ChildResult> {
        match self {
            Self::Running => None,
            Self::Completed(result)
            | Self::Aborted(result)
            | Self::Failed(result)
            | Self::Interrupted(result) => Some(result),
        }
    }

    pub(crate) fn result_mut(&mut self) -> Option<&mut ChildResult> {
        match self {
            Self::Running => None,
            Self::Completed(result)
            | Self::Aborted(result)
            | Self::Failed(result)
            | Self::Interrupted(result) => Some(result),
        }
    }

    pub(crate) fn notification(&self) -> Option<crate::tools::Event> {
        use crate::tools::{AbortCode, Event};
        let (label, result, code) = match self {
            Self::Running => return None,
            Self::Completed(result) => {
                return Some(Event::Assistant {
                    text: result.text.clone(),
                });
            }
            Self::Aborted(result) => ("aborted", result, AbortCode::Error),
            Self::Failed(result) => ("failed", result, AbortCode::Error),
            Self::Interrupted(result) => ("interrupted", result, AbortCode::Interrupted),
        };
        Some(Event::Aborted {
            reason: result
                .diagnostic
                .clone()
                .unwrap_or_else(|| label.to_string()),
            code,
        })
    }
}

impl ChildRecord {
    /// Projects the child's identity for live display and journal replay.
    pub fn spawned(&self) -> crate::tools::Event {
        use crate::tools::Event;
        Event::Child {
            id: self.id.to_string(),
            event: Box::new(Event::Spawned {
                assignment: self.assignment.clone(),
                access: self.access,
                call: self.call.clone().unwrap_or_default(),
                background: self.background,
                workspace: self.workspace.to_string_lossy().into_owned(),
                agent: self.agent.clone(),
                name: self.name.clone(),
                model: self.model.clone(),
            }),
        }
    }

    pub fn interrupt(&self, reason: &str) -> Self {
        let mut record = self.clone();
        record.state = ChildState::Interrupted(ChildResult::with_diagnostic(Some(reason)));
        record
    }

    pub(crate) fn same_child(&self, other: &Self) -> bool {
        self.version == other.version
            && self.id == other.id
            && self.parent == other.parent
            && self.assignment == other.assignment
            && self.model == other.model
            && self.name == other.name
            && self.agent == other.agent
            && self.workspace == other.workspace
            && self.depth == other.depth
            && self.access == other.access
            && self.background == other.background
            && self.base == other.base
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub struct ChildResult {
    pub text: String,
    pub changed_paths: Vec<String>,
    pub changed_paths_total: usize,
    pub rounds: usize,
    pub usage: Usage,
    pub billed_tokens: u64,
    pub elapsed_millis: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kept: Option<crate::tools::Kept>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub head: String,
}

impl ChildResult {
    pub(crate) fn with_diagnostic(diagnostic: Option<&str>) -> Self {
        Self {
            diagnostic: diagnostic.map(str::to_string),
            ..Self::default()
        }
    }
}

#[must_use]
pub fn tag(id: &str) -> String {
    let chars: Vec<char> = id.chars().collect();
    chars[chars.len().saturating_sub(8)..].iter().collect()
}
