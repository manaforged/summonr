use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::durable::log::{self, Line};
use crate::provider::Message;

mod catalog;
mod export;
mod profile;
mod prune;
mod sidecar;
mod usage;

pub use catalog::{info, last_for, list, resolve_id, short};
pub(crate) use catalog::{name_of as catalog_name, previous_for, prompts, roots};
pub(crate) use catalog::{remove, set_name};
pub use export::export;
pub use profile::{Scope as ProfileScope, profile};
pub use prune::prune;
pub(crate) use usage::Spend;
pub(crate) use usage::{DAY, HOUR, Total, buckets, spend};

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
}

const PEEK_LINES: usize = 64;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct Summary {
    cwd: Option<String>,
    model: Option<String>,
    preview: String,
    parent: Option<String>,
    depth: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    open_bytes: Option<u64>,
}

impl Summary {
    pub(crate) fn peek(path: &Path) -> (Self, bool) {
        if let Ok(head) = log::head(path, PEEK_LINES) {
            let (summary, found) = Self::from_lines(&head);
            if found {
                return (summary, true);
            }
        }
        match log::scan(path) {
            Ok(lines) => Self::from_lines(&lines.collect::<Vec<Line>>()),
            Err(_) => (Self::default(), false),
        }
    }

    fn from_lines(lines: &[Line]) -> (Self, bool) {
        let mut summary = Self::default();
        for line in lines {
            match line {
                Line::Session {
                    workspace,
                    model,
                    parent,
                    depth,
                    ..
                } => {
                    summary.cwd = Some(workspace.display().to_string());
                    summary.model = Some(model.clone());
                    summary.parent = parent.map(|id| id.to_string());
                    summary.depth = *depth;
                }
                Line::User { text, source, .. } if !text.is_empty() => {
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

    pub(crate) fn here(&self, roots: &[String]) -> bool {
        self.depth == 0
            && self
                .cwd
                .as_deref()
                .is_some_and(|cwd| roots.iter().any(|root| root == cwd))
    }

    pub(crate) fn into_info(self, id: String, path: PathBuf) -> SessionInfo {
        let name = catalog::name_of(&path);
        SessionInfo {
            id,
            path,
            cwd: self.cwd,
            model: self.model,
            preview: self.preview,
            name,
            parent: self.parent,
            depth: self.depth,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Prompt {
    pub session: Arc<SessionInfo>,
    pub turn: usize,
    pub text: String,
    pub images: usize,
    pub closed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Replay {
    pub history: Vec<Message>,
    pub prompt: Option<String>,
    pub model: Option<String>,
    pub id: String,
    pub spent_tokens: u64,
    pub elapsed_millis: u64,
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
