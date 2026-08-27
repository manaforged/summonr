//! Session catalog and replay.

use std::path::PathBuf;

use crate::provider::Message;

mod catalog;

pub use catalog::{last_for, list, resolve_id};

/// One listed session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: String,
    pub path: PathBuf,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub preview: String,
}

/// History reconstructed from the session log.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Replay {
    pub history: Vec<Message>,
    pub prompt: Option<String>,
    pub model: Option<String>,
    pub id: String,
    pub spent_tokens: u64,
    pub elapsed_millis: u64,
}

pub(crate) fn decorate(harness: &str, text: &str) -> String {
    if text.is_empty() {
        harness.to_string()
    } else {
        format!("{text}\n\n{harness}")
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
