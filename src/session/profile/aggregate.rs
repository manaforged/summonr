use std::collections::BTreeMap;

use crate::durable::log::{self, Line};
use crate::error::Result;

use super::fmt::{count, line, secs, share};
use super::health::Health;

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
    let mut health = Health::default();
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
                    ..
                } => {
                    index += 1;
                    calls += 1;
                    if !success {
                        failed += 1;
                    }
                    millis = millis.saturating_add(ms);
                    chars = chars.saturating_add(text.chars().count() as u64);
                    let (name, arguments) = pending.remove(&call).unwrap_or_default();
                    if health.add(&name, &arguments, success) && !edited {
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
                Line::Close { footprint_kb, .. } => {
                    if let Some(kb) = footprint_kb {
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
    for text in health.lines() {
        line(&mut out, text);
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
                "mem      median {:.0} MB · max {:.0} MB",
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
