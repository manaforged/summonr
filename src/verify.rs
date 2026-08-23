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

#[cfg(test)]
#[path = "verify_tests.rs"]
mod tests;
