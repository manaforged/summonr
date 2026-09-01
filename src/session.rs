use std::path::PathBuf;

use crate::provider::Message;

mod catalog;

#[cfg(test)]
pub(crate) use catalog::list_in;
pub use catalog::{export, last_for, list, resolve_id, set_name, short};
pub(crate) use catalog::{name_of as catalog_name, previous_for};

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
