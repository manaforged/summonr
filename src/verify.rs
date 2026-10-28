use std::collections::BTreeMap;

pub(crate) use summonr_types::Outcome;
pub use summonr_types::Receipt;

use crate::tools::{Event, ToolOutput};
use summonr_core::error::Error;

mod file;
mod metric;
pub use file::check_commands;
pub(crate) use file::{check_path, metric};
pub(crate) use metric::{Metric, Protected};

#[must_use]
pub(crate) fn new(changes: Vec<String>, checks: Vec<Outcome>, snapshot: String) -> Receipt {
    let verified = !checks.is_empty() && checks.iter().all(|check| check.passed);
    Receipt {
        changes,
        checks,
        snapshot,
        verified,
        ..Receipt::default()
    }
}

#[must_use]
pub(crate) fn from_turn(
    changes: Vec<String>,
    snapshot: String,
    events: &[Event],
    configured: &[String],
) -> Receipt {
    let checks = ran(events, configured);
    new(changes, checks, snapshot)
}

#[must_use]
pub(crate) fn aborted(snapshot: String) -> Receipt {
    let mut receipt = new(Vec::new(), Vec::new(), snapshot);
    receipt.aborted = true;
    receipt
}

#[must_use]
pub(crate) fn outcome(command: &str, result: &std::result::Result<ToolOutput, Error>) -> Outcome {
    let (passed, output) = match result {
        Ok(output) => (output.success, output.text.clone()),
        Err(error) => (false, error.to_string()),
    };
    Outcome {
        command: command.to_string(),
        passed,
        output,
    }
}

#[must_use]
pub(crate) fn context(receipt: &Receipt) -> String {
    let failed = receipt.checks.iter().any(|check| !check.passed);
    let mut text = String::new();
    if failed {
        text.push_str("Verification failed.\n\n");
    }
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

pub(crate) fn ran(events: &[Event], configured: &[String]) -> Vec<Outcome> {
    let mut calls = BTreeMap::new();
    for event in events {
        if let Event::ToolCall {
            id,
            name,
            arguments,
        } = event
            && crate::tools::Builtin::of(name) == Some(crate::tools::Builtin::RunCommand)
            && let Some(command) = command_of(arguments)
            && is_check(&command, configured)
        {
            calls.insert(id.as_str(), command);
        }
    }
    let mut outcomes = Vec::new();
    for event in events {
        if let Event::ToolResult {
            id,
            success,
            output,
            ..
        } = event
            && let Some(command) = calls.get(id.as_str())
        {
            outcomes.push(Outcome {
                command: (*command).clone(),
                passed: *success,
                output: output.clone(),
            });
        }
    }
    outcomes
}

fn command_of(arguments: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(arguments)
        .ok()?
        .get("command")?
        .as_str()
        .map(str::to_string)
}

fn is_check(command: &str, configured: &[String]) -> bool {
    configured.iter().any(|check| check == command) || command.contains("loop verify")
}

fn code(text: &str) -> String {
    format!("`{}`", text.replace('`', "\\`"))
}

#[cfg(test)]
#[path = "verify_tests.rs"]
mod tests;
