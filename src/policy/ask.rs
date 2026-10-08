use std::path::Path;

use super::{Policy, danger, paths::matches_deny};

impl Policy {
    #[must_use]
    pub(crate) fn needs_approval(
        &self,
        command: &str,
        workspace: &Path,
        start: &Path,
    ) -> Option<String> {
        if let Some(reason) = danger(command, workspace, start, self.confirm) {
            return Some(reason.to_string());
        }
        self.ask
            .iter()
            .find(|pattern| matches_deny(command, pattern))
            .map(|pattern| format!("it matches ask pattern `{pattern}`"))
    }
}

pub(super) fn extend_unique(list: &mut Vec<String>, items: Vec<String>) {
    for item in items {
        if !item.trim().is_empty() && !list.iter().any(|have| have == &item) {
            list.push(item);
        }
    }
}

#[cfg(test)]
#[path = "ask_tests.rs"]
mod tests;
