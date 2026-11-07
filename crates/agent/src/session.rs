use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::durable::log::{self, Line};

pub(crate) const PARENT_ENV: &str = "SUMMONR_PARENT_SESSION";
pub(crate) const SESSION_ENV: &str = "SUMMONR_SESSION_ID";

static INTERACTIVE: AtomicBool = AtomicBool::new(false);

/// Marks the top-level sessions this process opens as started by a person in the terminal or window.
pub fn mark_interactive() {
    INTERACTIVE.store(true, Ordering::Relaxed);
}

pub(crate) fn interactive() -> bool {
    INTERACTIVE.load(Ordering::Relaxed)
}

mod catalog;
mod export;
mod profile;
mod prune;
pub(crate) mod sidecar;
mod usage;

pub use catalog::{all, remove, set_name};
pub use catalog::{info, last_for, list, resolve_id, short};
pub(crate) use catalog::{name_of as catalog_name, previous_for, prompts, roots};
pub use export::{export, export_json};
pub use profile::{Scope as ProfileScope, profile};
pub use prune::prune;
pub use usage::Spend;
pub use usage::{DAY, HOUR, Total, buckets, spend};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: String,
    pub path: PathBuf,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub preview: String,
    pub name: Option<String>,
    pub parent: Option<String>,
    pub depth: u8,
    pub interactive: bool,
    /// The route of the foreign agent that ran this chat's first message.
    pub agent: Option<String>,
    /// The thread that started this one's topic, when it joined another thread's topic.
    pub topic: Option<String>,
    /// When the session journal last changed.
    pub modified: Option<std::time::SystemTime>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct Summary {
    cwd: Option<String>,
    model: Option<String>,
    preview: String,
    parent: Option<String>,
    depth: u8,
    #[serde(default)]
    interactive: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    topic: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    open_bytes: Option<u64>,
}

impl Summary {
    pub(crate) fn peek(path: &Path) -> (Self, bool) {
        let mut summary = Self::default();
        for line in log::records::<Line>(path, 0).filter_map(Result::ok) {
            match &line {
                Line::Session {
                    workspace,
                    model,
                    parent,
                    depth,
                    interactive,
                    ..
                } => {
                    summary.interactive = *interactive;
                    summary.cwd = Some(workspace.display().to_string());
                    summary.model = Some(model.clone());
                    summary.parent = parent.map(|id| id.to_string());
                    summary.depth = *depth;
                }
                Line::Agent { route } => summary.agent = Some(route.clone()),
                Line::Topic { id } => summary.topic = Some(id.clone()),
                Line::User {
                    text,
                    source,
                    start,
                    ..
                } if !text.is_empty() && (*start || source.is_some()) => {
                    summary.preview = catalog::preview(text, source.as_deref());
                    return (summary, true);
                }
                _ => {}
            }
        }
        (summary, false)
    }

    pub(crate) fn settled(&self) -> bool {
        self.open_bytes.is_none()
    }

    pub(crate) fn open_bytes(&self) -> Option<u64> {
        self.open_bytes
    }

    pub(crate) fn mark_open(&mut self, bytes: Option<u64>) {
        self.open_bytes = bytes;
    }

    pub(crate) fn in_workspace(&self, roots: &[String]) -> bool {
        self.cwd
            .as_deref()
            .is_some_and(|cwd| roots.iter().any(|root| root == cwd))
    }

    pub(crate) fn here(&self, roots: &[String]) -> bool {
        self.depth == 0 && self.in_workspace(roots)
    }

    pub(crate) fn into_info(self, id: String, path: PathBuf) -> SessionInfo {
        let name = catalog::name_of(&path);
        let modified = std::fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok();
        SessionInfo {
            id,
            path,
            cwd: self.cwd,
            model: self.model,
            preview: self.preview,
            name,
            parent: self.parent,
            depth: self.depth,
            interactive: self.interactive,
            agent: self.agent,
            topic: self.topic,
            modified,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub session: Arc<SessionInfo>,
    pub turn: usize,
    pub text: String,
    pub images: usize,
    pub closed: bool,
}

pub(crate) fn decorate(harness: &str, text: &str) -> String {
    if harness.is_empty() {
        text.to_string()
    } else if text.is_empty() {
        harness.to_string()
    } else {
        format!("{text}\n\n{harness}")
    }
}

const HARNESS_HEADERS: [&str; 5] = [
    "Harness:",
    "Harness state",
    "Harness evidence",
    "Harness ledger",
    "Harness objective",
];

pub(crate) fn user_text<'a>(text: &'a str, source: Option<&'a str>) -> &'a str {
    source.unwrap_or_else(|| {
        text.match_indices("\n\nHarness")
            .find(|(at, _)| {
                HARNESS_HEADERS
                    .iter()
                    .any(|header| text[at + 2..].starts_with(header))
            })
            .map_or(text, |(at, _)| &text[..at])
    })
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
