use std::collections::{BTreeMap, BTreeSet};

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
pub(crate) fn new(
    changes: Vec<String>,
    checks: Vec<Outcome>,
    snapshot: String,
    events: &[Event],
) -> Receipt {
    let verified = !checks.is_empty() && checks.iter().all(|check| check.passed);
    let mut receipt = Receipt {
        changes,
        checks,
        snapshot,
        verified,
        ..Receipt::default()
    };
    tally(events, &mut receipt);
    receipt.text = render(&receipt, true);
    receipt
}

#[must_use]
pub(crate) fn from_turn(
    changes: Vec<String>,
    snapshot: String,
    events: &[Event],
    configured: &[String],
) -> Receipt {
    let checks = ran(events, configured);
    let mut receipt = new(changes, checks, snapshot, events);
    if receipt.checks.is_empty() {
        receipt.text = render(&receipt, false);
    }
    receipt
}

#[must_use]
pub(crate) fn aborted(snapshot: String, events: &[Event]) -> Receipt {
    let mut receipt = new(Vec::new(), Vec::new(), snapshot, events);
    receipt.aborted = true;
    receipt.text = render(&receipt, true);
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
                if let Some(path) = tally_call(name, arguments, receipt) {
                    read_calls.insert(id.as_str(), path);
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

fn tally_call(name: &str, arguments: &str, receipt: &mut Receipt) -> Option<String> {
    let kind = crate::tools::Builtin::of(name);
    let read = kind == Some(crate::tools::Builtin::Read);
    let op = crate::tools::retired_op(name);
    if matches!(
        kind,
        Some(crate::tools::Builtin::Web | crate::tools::Builtin::Search)
    ) || (read && matches!(op, Some("search" | "list")))
    {
        receipt.searches += 1;
    } else if kind == Some(crate::tools::Builtin::RunCommand) {
        receipt.commands += 1;
    } else if read
        && matches!(op, None | Some("file"))
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(arguments)
    {
        return tally_read(&value, receipt);
    }
    None
}

fn tally_read(value: &serde_json::Value, receipt: &mut Receipt) -> Option<String> {
    let pattern = value
        .get("pattern")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|pattern| !pattern.is_empty());
    let glob = value
        .get("glob")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|glob| !glob.trim().is_empty());
    if pattern || glob {
        receipt.searches += 1;
        return None;
    }
    value
        .get("path")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

fn render(receipt: &Receipt, checked: bool) -> String {
    let cap = crate::limits::limits().receipt_items_max;
    let mut text = format!(
        "Turn receipt · {}\n- Started at: {}",
        headline(receipt),
        receipt.snapshot
    );
    if !receipt.failures.is_empty() {
        let mut seen = BTreeSet::new();
        let failures: Vec<String> = receipt
            .failures
            .iter()
            .filter(|failure| seen.insert(failure.as_str()))
            .cloned()
            .collect();
        text.push_str(&format!("\n- Failed: {}", capped(&failures, cap, "; ")));
    }
    if receipt.aborted {
        text.push_str("\n- Changed: not captured before abort\n- Verification: not run");
        return text;
    }
    if !receipt.changes.is_empty() {
        let names: Vec<String> = receipt.changes.iter().map(|change| code(change)).collect();
        text.push_str(&format!("\n- Changed: {}", capped(&names, cap, ", ")));
    }
    if !checked {
        text.push_str("\n- Verification: not run");
        return text;
    }
    let checks = summary(&[], &receipt.checks);
    if let Some(at) = checks.find("\n- Verification:") {
        text.push_str(&checks[at..]);
    }
    text
}

fn headline(receipt: &Receipt) -> String {
    let mut parts = vec![
        format!("{} read", receipt.reads),
        summonr_core::text::plural(receipt.searches, "search"),
        summonr_core::text::plural(receipt.commands, "command"),
    ];
    if !receipt.failures.is_empty() {
        parts.push(format!("{} failed", receipt.failures.len()));
    }
    parts.push(if receipt.aborted {
        "aborted".to_string()
    } else if receipt.changes.is_empty() {
        "no changes".to_string()
    } else {
        format!("{} changed", receipt.changes.len())
    });
    parts.push(
        match (
            receipt.checks.is_empty(),
            receipt.checks.iter().all(|check| check.passed),
        ) {
            _ if receipt.aborted => "not verified",
            (true, _) => "no check",
            (false, true) => "check passed",
            (false, false) => "check failed",
        }
        .to_string(),
    );
    parts.join(" · ")
}

fn capped(items: &[String], cap: usize, separator: &str) -> String {
    let mut shown = items
        .iter()
        .take(cap)
        .cloned()
        .collect::<Vec<_>>()
        .join(separator);
    if items.len() > cap {
        shown.push_str(&format!("{separator}+{} more", items.len() - cap));
    }
    shown
}

fn code(text: &str) -> String {
    format!("`{}`", text.replace('`', "\\`"))
}

#[cfg(test)]
#[path = "verify_tests.rs"]
mod tests;
