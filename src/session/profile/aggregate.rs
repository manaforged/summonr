use std::collections::BTreeMap;

use crate::durable::log::{self, Line, Usage};
use summonr_core::error::Result;
use summonr_types::ToolCall;

use super::fmt::{count, line, secs, share};
use super::health::Health;

pub(super) fn profile_all(last: Option<usize>) -> Result<String> {
    let home = summonr_core::dirs::summonr_home()?;
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
        let mut scan = SessionScan::default();
        match log::records(path, 0).collect::<std::io::Result<Vec<_>>>() {
            Ok(lines) => {
                for line in lines {
                    totals.add_line(&mut scan, line);
                }
                totals.read_sessions += 1;
            }
            Err(_) => totals.skipped += 1,
        }
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
    usage: Usage,
    warm: Usage,
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
    fn add_line(&mut self, scan: &mut SessionScan, line: Line) {
        match line {
            Line::Assistant {
                tool_calls, usage, ..
            } => self.add_assistant(scan, tool_calls, &usage),
            Line::Tool {
                call,
                text,
                success,
                millis,
                ..
            } => self.add_tool(scan, &call, &text, success, millis),
            Line::Close { footprint_kb, .. } => self.rss.extend(footprint_kb),
            _ => {}
        }
    }

    fn add_assistant(&mut self, scan: &mut SessionScan, tool_calls: Vec<ToolCall>, usage: &Usage) {
        self.usage.add(*usage);
        if scan.warm {
            self.warm.add(*usage);
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
                count(self.usage.input),
                share(self.usage.cached, self.usage.input),
                share(
                    self.usage.cached.saturating_sub(self.warm.cached),
                    self.usage.input.saturating_sub(self.warm.input)
                ),
                share(self.warm.cached, self.warm.input),
                count(self.usage.input.saturating_sub(self.usage.cached)),
                count(self.usage.output)
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
                    summonr_core::text::megabytes(median),
                    summonr_core::text::megabytes(max)
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
