use std::path::Path;

use super::{Gate, Policy, danger, paths::matches_deny};

impl Policy {
    pub(crate) fn toggle_approval(&mut self) -> bool {
        self.session_confirm = match self.session_confirm {
            Gate::Destructive => Gate::Risky,
            Gate::Risky => Gate::Destructive,
        };
        self.is_approval()
    }

    pub(crate) fn is_approval(&self) -> bool {
        self.confirm.max(self.session_confirm) == Gate::Risky
    }

    #[must_use]
    pub(crate) fn needs_approval(
        &self,
        command: &str,
        workspace: &Path,
        start: &Path,
    ) -> Option<String> {
        if let Some(reason) = danger(
            command,
            workspace,
            start,
            self.confirm.max(self.session_confirm),
        ) {
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
