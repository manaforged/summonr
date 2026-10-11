use std::path::PathBuf;
use std::sync::Arc;

use crate::provider::Message;

mod catalog;
mod export;
mod profile;
mod prune;
mod usage;

pub use catalog::{last_for, list, resolve_id, short};
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

pub(crate) fn user_text(text: &str) -> &str {
    text.split("\n\nHarness").next().unwrap_or(text)
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
