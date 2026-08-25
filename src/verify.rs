//! Check command configuration.
//!
//! `<workspace>/.mage/check.toml` names one command or several:
//!
//! ```toml
//! check = "cargo test -p summoner"
//! # or:
//! checks = ["cargo clippy -- -D warnings", "cargo test -p summoner"]
//! ```

use std::path::Path;

use serde::Deserialize;

use crate::error::{Error, Result};
use crate::tools::ToolOutput;

/// Verdict of one verify run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub command: String,
    pub passed: bool,
    pub output: String,
}

#[derive(Debug, Default, Deserialize)]
struct CheckFile {
    check: Option<String>,
    checks: Option<Vec<String>>,
}

/// Load the workspace check commands. `Ok(None)` when unconfigured.
pub fn check_commands(workspace: &Path) -> Result<Option<Vec<String>>> {
    let path = workspace.join(".mage/check.toml");
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)?;
    let file: CheckFile = toml::from_str(&text)
        .map_err(|error| Error::Config(format!("{}: {error}", path.display())))?;
    if file.check.is_some() && file.checks.is_some() {
        return Err(Error::Config(format!(
            "{}: use `check` or `checks`, not both",
            path.display()
        )));
    }
    let mut commands = match (file.check, file.checks) {
        (Some(check), _) => vec![check],
        (None, Some(checks)) => checks,
        (None, None) => Vec::new(),
    };
    commands.retain(|command| !command.trim().is_empty());
    if commands.is_empty() {
        return Err(Error::Config(format!(
            "{}: `check` must name at least one command",
            path.display()
        )));
    }
    Ok(Some(commands))
}

/// Convert the command executor result into a completion-check verdict.
#[must_use]
pub(crate) fn outcome(command: &str, result: &std::result::Result<ToolOutput, Error>) -> Outcome {
    match result {
        Ok(output) => Outcome {
            command: command.to_string(),
            passed: output.success,
            output: output.text.clone(),
        },
        Err(error) => Outcome {
            command: command.to_string(),
            passed: false,
            output: error.to_string(),
        },
    }
}

/// Append controller-owned mutation and verification facts to provider prose.
#[must_use]
pub(crate) fn receipt(model: &str, changes: &[String], checks: &[Outcome]) -> String {
    let failed = checks.iter().find(|check| !check.passed);
    let mut text = String::new();
    if failed.is_some() {
        text.push_str("Verification failed.\n\n");
    }
    text.push_str(model.trim());
    text.push_str("\n\n");
    text.push_str(&summary(changes, checks));
    text
}

/// Format the harness-owned part of a turn without repeating model prose.
#[must_use]
pub(crate) fn summary(changes: &[String], checks: &[Outcome]) -> String {
    let failed = checks.iter().find(|check| !check.passed);
    let mut text = String::from("Turn receipt\n- Changed: ");
    if changes.is_empty() {
        text.push_str("none");
    } else {
        text.push_str(
            &changes
                .iter()
                .map(|change| code(change))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    if checks.is_empty() {
        text.push_str("\n- Verification: not configured");
    } else {
        for check in checks {
            text.push_str("\n- Verification: ");
            text.push_str(&code(&check.command));
            text.push(' ');
            text.push_str(if check.passed { "passed" } else { "failed" });
        }
    }
    if let Some(check) = failed
        && !check.output.trim().is_empty()
    {
        text.push_str("\n\nCheck output\n");
        for line in check.output.trim().lines() {
            text.push_str("    ");
            text.push_str(line);
            text.push('\n');
        }
        while text.ends_with('\n') {
            text.pop();
        }
    }
    text
}

fn code(text: &str) -> String {
    format!("`{}`", text.replace('`', "\\`"))
}

#[cfg(test)]
#[path = "verify_tests.rs"]
mod tests;
