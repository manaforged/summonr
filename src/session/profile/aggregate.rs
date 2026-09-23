use std::collections::BTreeMap;

use crate::durable::log::{self, Line};
use crate::error::Result;

use super::{count, head_tail, line, secs, share};

pub(super) fn profile_all(last: Option<usize>) -> Result<String> {
    let home = crate::prompt::summonr_home()?;
    let mut sessions = crate::session::catalog::list_in(&home)?;
    if let Some(last) = last {
        let skip = sessions.len().saturating_sub(last);
        sessions.drain(..skip);
    }
    let mut read_sessions = 0_usize;
    let mut skipped = 0_usize;
    let mut calls = 0_usize;
    let mut failed = 0_usize;
    let mut millis = 0_u64;
    let mut chars = 0_u64;
    let mut input = 0_u64;
    let mut output = 0_u64;
    let mut cached = 0_u64;
    let mut warm_input = 0_u64;
    let mut warm_cached = 0_u64;
    let mut per_tool: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut native = 0_usize;
    let mut shell = 0_usize;
    let mut commands = 0_usize;
    let mut pipes = 0_usize;
    let mut first_edit: Vec<usize> = Vec::new();
    let mut rss: Vec<u64> = Vec::new();
    for session in &sessions {
        let mut pending: BTreeMap<String, (String, String)> = BTreeMap::new();
        let mut index = 0_usize;
        let mut edited = false;
        let mut first = true;
        let lines = match log::read(&session.path) {
            Ok(lines) => lines,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        read_sessions += 1;
        for line in lines {
            match line {
                Line::Assistant {
                    tool_calls, usage, ..
                } => {
                    input = input.saturating_add(usage.input);
                    output = output.saturating_add(usage.output);
                    cached = cached.saturating_add(usage.cached);
                    if first {
                        first = false;
                    } else {
                        warm_input = warm_input.saturating_add(usage.input);
                        warm_cached = warm_cached.saturating_add(usage.cached);
                    }
                    for call in tool_calls {
                        pending.insert(call.id, (call.function.name, call.function.arguments));
                    }
                }
                Line::Tool {
                    call,
                    text,
                    success,
                    millis: ms,
                } => {
                    index += 1;
                    calls += 1;
                    if !success {
                        failed += 1;
                    }
                    millis = millis.saturating_add(ms);
                    chars = chars.saturating_add(text.chars().count() as u64);
                    let (name, arguments) = pending.remove(&call).unwrap_or_default();
                    let entry = per_tool.entry(name.clone()).or_default();
                    entry.0 += 1;
                    if !success {
                        entry.1 += 1;
                    }
                    let args: serde_json::Value =
                        serde_json::from_str(&arguments).unwrap_or_default();
                    if name == "run_command" {
                        commands += 1;
                        let command = args
                            .get("command")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default();
                        if crate::tools::command::searchish(command) {
                            shell += 1;
                        }
                        if head_tail(command) {
                            pipes += 1;
                        }
                    } else if name == "read" {
                        let op = args
                            .get("op")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default();
                        let pattern = args
                            .get("pattern")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default();
                        if op == "search" || (op.is_empty() && !pattern.is_empty()) {
                            native += 1;
                        }
                    } else if name == "grep" {
                        native += 1;
                    }
                    if !edited && success && matches!(name.as_str(), "edit" | "write") {
                        edited = true;
                        first_edit.push(index);
                    }
                }
                Line::Tools { .. }
                | Line::Session { .. }
                | Line::User { .. }
                | Line::Charge { .. }
                | Line::Budget { .. }
                | Line::Workflow { .. }
                | Line::Child { .. }
                | Line::Interrupted { .. }
                | Line::Receipt { .. }
                | Line::Summary { .. }
                | Line::Rewrite { .. }
                | Line::Clear => {}
                Line::Close { rss_kb, .. } => {
                    if let Some(kb) = rss_kb {
                        rss.push(kb);
                    }
                }
            }
        }
    }
    let mut out = String::new();
    line(
        &mut out,
        format!(
            "{} sessions ({} skipped unreadable) · {calls} tool calls · {} failed ({}) · {} tool time",
            read_sessions,
            skipped,
            failed,
            share(failed as u64, calls as u64),
            secs(millis),
        ),
    );
    line(
        &mut out,
        format!(
            "tokens  in {} ({} cached · cold {} hit · warm {} hit · uncached {}) · out {}",
            count(input),
            share(cached, input),
            share(
                cached.saturating_sub(warm_cached),
                input.saturating_sub(warm_input)
            ),
            share(warm_cached, warm_input),
            count(input.saturating_sub(cached)),
            count(output)
        ),
    );
    let mut tools: Vec<String> = per_tool
        .iter()
        .map(|(name, (calls, failed))| {
            if *failed > 0 {
                format!("{name} {calls} ({failed} failed)")
            } else {
                format!("{name} {calls}")
            }
        })
        .collect();
    tools.sort();
    if !tools.is_empty() {
        line(&mut out, format!("calls    {}", tools.join(" · ")));
    }
    if native + shell > 0 {
        line(
            &mut out,
            format!("search   native {native} · shell {shell}"),
        );
    }
    if commands > 0 {
        line(
            &mut out,
            format!(
                "pipes    {pipes} of {commands} run_command calls pipe to head/tail ({})",
                share(pipes as u64, commands as u64)
            ),
        );
    }
    if calls > 0 {
        line(
            &mut out,
            format!(
                "size     {} chars per tool call",
                count(chars / calls as u64)
            ),
        );
    }
    rss.sort_unstable();
    if !rss.is_empty() {
        let median = rss[rss.len() / 2];
        let max = rss[rss.len() - 1];
        line(
            &mut out,
            format!(
                "rss      median {:.0} MB · max {:.0} MB",
                median as f64 / 1024.0,
                max as f64 / 1024.0
            ),
        );
    }
    first_edit.sort_unstable();
    if !first_edit.is_empty() {
        let median = first_edit[first_edit.len() / 2];
        line(
            &mut out,
            format!(
                "edits    {} sessions edited · median first edit at call {median}",
                first_edit.len()
            ),
        );
    }
    Ok(out)
}
