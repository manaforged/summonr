use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::tools::{Event, ToolOutput};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Outcome {
    pub command: String,
    pub passed: bool,
    pub output: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Receipt {
    pub changes: Vec<String>,
    pub checks: Vec<Outcome>,
    #[serde(default)]
    pub snapshot: String,
    #[serde(default)]
    pub reads: usize,
    #[serde(default)]
    pub searches: usize,
    #[serde(default)]
    pub commands: usize,
    #[serde(default)]
    pub failures: Vec<String>,
    #[serde(default)]
    pub aborted: bool,
    #[serde(default)]
    pub verified: bool,
    pub text: String,
}

impl Receipt {
    #[must_use]
    pub(crate) fn new(
        changes: Vec<String>,
        checks: Vec<Outcome>,
        snapshot: String,
        events: &[Event],
    ) -> Self {
        let verified = !checks.is_empty() && checks.iter().all(|check| check.passed);
        let mut receipt = Self {
            changes,
            checks,
            snapshot,
            reads: 0,
            searches: 0,
            commands: 0,
            failures: Vec::new(),
            aborted: false,
            verified,
            text: String::new(),
        };
        tally(events, &mut receipt);
        receipt.text = render(&receipt);
        receipt
    }

    #[must_use]
    pub(crate) fn aborted(snapshot: String, events: &[Event]) -> Self {
        let mut receipt = Self::new(Vec::new(), Vec::new(), snapshot, events);
        receipt.aborted = true;
        receipt.text = render(&receipt);
        receipt
    }
}

#[derive(Debug, Default, Deserialize)]
struct CheckFile {
    check: Option<String>,
    checks: Option<Vec<String>>,
}

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

#[must_use]
pub(crate) fn context(model: &str, receipt: &Receipt) -> String {
    let failed = receipt.checks.iter().any(|check| !check.passed);
    let mut text = String::new();
    if failed {
        text.push_str("Verification failed.\n\n");
    }
    text.push_str(model.trim());
    text.push_str("\n\n");
    text.push_str(&receipt.text);
    text
}

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

fn tally(events: &[Event], receipt: &mut Receipt) {
    let mut reads = BTreeSet::new();
    let mut calls = BTreeMap::new();
    let mut read_calls = BTreeMap::new();
    for event in events {
        match event {
            Event::ToolCall {
                id,
                name,
                arguments,
            } => {
                calls.insert(id.as_str(), name.as_str());
                if name == "read_file"
                    && let Ok(value) = serde_json::from_str::<serde_json::Value>(arguments)
                    && let Some(path) = value.get("path").and_then(serde_json::Value::as_str)
                {
                    read_calls.insert(id.as_str(), path.to_string());
                }
                if matches!(
                    name.as_str(),
                    "grep" | "list_dir" | "web_search" | "web_fetch"
                ) {
                    receipt.searches += 1;
                } else if name == "run_command" {
                    receipt.commands += 1;
                }
            }
            Event::ToolResult {
                id, success: true, ..
            } => {
                if let Some(path) = read_calls.get(id.as_str()) {
                    reads.insert(path.clone());
                }
            }
            Event::ToolResult {
                id,
                success: false,
                output,
                ..
            } => {
                let name = calls.get(id.as_str()).copied().unwrap_or("tool");
                let reason = output
                    .lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or("failed")
                    .to_string();
                receipt.failures.push(format!("{name}: {reason}"));
            }
            _ => {}
        }
    }
    receipt.reads = reads.len();
}

fn render(receipt: &Receipt) -> String {
    let mut text = if receipt.aborted {
        "Turn receipt\n- Changed: not captured before abort\n- Verification: not run".to_string()
    } else {
        summary(&receipt.changes, &receipt.checks)
    };
    let file = if receipt.reads == 1 { "file" } else { "files" };
    let search = if receipt.searches == 1 {
        "search"
    } else {
        "searches"
    };
    let command = if receipt.commands == 1 {
        "command"
    } else {
        "commands"
    };
    let header = format!(
        "Turn receipt\n- Started at: {}\n- Inspected: {} {file} read, {} {search}, {} {command}",
        receipt.snapshot, receipt.reads, receipt.searches, receipt.commands
    );
    text.replace_range(.."Turn receipt".len(), &header);
    if !receipt.failures.is_empty() {
        let shown = receipt.failures.join("; ");
        let at = text.find("\n- Changed:").unwrap_or(text.len());
        text.insert_str(
            at,
            &format!("\n- Failed: {} — {shown}", receipt.failures.len()),
        );
    }
    text
}

fn code(text: &str) -> String {
    format!("`{}`", text.replace('`', "\\`"))
}

#[cfg(test)]
#[path = "verify_tests.rs"]
mod tests;
