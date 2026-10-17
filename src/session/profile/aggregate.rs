use std::collections::BTreeMap;

use crate::durable::log::{self, Line, Usage};
use crate::error::Result;
use crate::provider::ToolCall;

use super::fmt::{count, line, secs, share};
use super::health::Health;

pub(super) fn profile_all(last: Option<usize>) -> Result<String> {
    let home = crate::prompt::summonr_home()?;
    let mut paths: Vec<_> = crate::session::catalog::ids_in(&home)?
        .into_iter()
        .rev()
        .map(|(_, path)| path)
        .filter(|path| path.is_file())
        .take(last.unwrap_or(usize::MAX))
        .collect();
    paths.reverse();
    let mut totals = Totals::default();
    for path in &paths {
        let lines = match log::read(path) {
            Ok(lines) => lines,
            Err(_) => {
                totals.skipped += 1;
                continue;
            }
        };
        totals.read_sessions += 1;
        totals.add_session(lines);
    }
    Ok(totals.render())
}

#[derive(Default)]
struct Totals {
    read_sessions: usize,
    skipped: usize,
    calls: usize,
    failed: usize,
    millis: u64,
    chars: u64,
    input: u64,
    output: u64,
    cached: u64,
    warm_input: u64,
    warm_cached: u64,
    health: Health,
    first_edit: Vec<usize>,
    rss: Vec<u64>,
}

#[derive(Default)]
struct SessionScan {
    pending: BTreeMap<String, (String, String)>,
    index: usize,
    edited: bool,
    warm: bool,
}

impl Totals {
    fn add_session(&mut self, lines: Vec<Line>) {
        let mut scan = SessionScan::default();
        for line in lines {
            match line {
                Line::Assistant {
                    tool_calls, usage, ..
                } => self.add_assistant(&mut scan, tool_calls, &usage),
                Line::Tool {
                    call,
                    text,
                    success,
                    millis,
                    ..
                } => self.add_tool(&mut scan, &call, &text, success, millis),
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
                Line::Close { footprint_kb, .. } => self.rss.extend(footprint_kb),
            }
        }
    }

    fn add_assistant(&mut self, scan: &mut SessionScan, tool_calls: Vec<ToolCall>, usage: &Usage) {
        self.input = self.input.saturating_add(usage.input);
        self.output = self.output.saturating_add(usage.output);
        self.cached = self.cached.saturating_add(usage.cached);
        if scan.warm {
            self.warm_input = self.warm_input.saturating_add(usage.input);
            self.warm_cached = self.warm_cached.saturating_add(usage.cached);
        } else {
            scan.warm = true;
        }
        for call in tool_calls {
            scan.pending
                .insert(call.id, (call.function.name, call.function.arguments));
        }
    }

    fn add_tool(
        &mut self,
        scan: &mut SessionScan,
        call: &str,
        text: &str,
        success: bool,
        millis: u64,
    ) {
        scan.index += 1;
        self.calls += 1;
        if !success {
            self.failed += 1;
        }
        self.millis = self.millis.saturating_add(millis);
        self.chars = self.chars.saturating_add(text.chars().count() as u64);
        let (name, arguments) = scan.pending.remove(call).unwrap_or_default();
        if self.health.add(&name, &arguments, success) && !scan.edited {
            scan.edited = true;
            self.first_edit.push(scan.index);
        }
    }

    fn render(mut self) -> String {
        let mut out = String::new();
        line(
            &mut out,
            format!(
                "{} sessions ({} skipped unreadable) · {} tool calls · {} failed ({}) · {} tool time",
                self.read_sessions,
                self.skipped,
                self.calls,
                self.failed,
                share(self.failed as u64, self.calls as u64),
                secs(self.millis),
            ),
        );
        line(
            &mut out,
            format!(
                "tokens  in {} ({} cached · cold {} hit · warm {} hit · uncached {}) · out {}",
                count(self.input),
                share(self.cached, self.input),
                share(
                    self.cached.saturating_sub(self.warm_cached),
                    self.input.saturating_sub(self.warm_input)
                ),
                share(self.warm_cached, self.warm_input),
                count(self.input.saturating_sub(self.cached)),
                count(self.output)
            ),
        );
        for text in self.health.lines() {
            line(&mut out, text);
        }
        if self.calls > 0 {
            line(
                &mut out,
                format!(
                    "size     {} chars per tool call",
                    count(self.chars / self.calls as u64)
                ),
            );
        }
        self.rss.sort_unstable();
        if !self.rss.is_empty() {
            let median = self.rss[self.rss.len() / 2];
            let max = self.rss[self.rss.len() - 1];
            line(
                &mut out,
                format!(
                    "mem      median {} · max {}",
                    crate::text::megabytes(median),
                    crate::text::megabytes(max)
                ),
            );
        }
        self.first_edit.sort_unstable();
        if !self.first_edit.is_empty() {
            let median = self.first_edit[self.first_edit.len() / 2];
            line(
                &mut out,
                format!(
                    "edits    {} sessions edited · median first edit at call {median}",
                    self.first_edit.len()
                ),
            );
        }
        out
    }
}
