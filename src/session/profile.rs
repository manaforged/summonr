use std::collections::BTreeMap;
use std::path::Path;

use crate::durable::log::{self, Line};
use crate::error::Result;

const UNACCOUNTED: &str = "unaccounted turn time";
const TOP: usize = 5;

struct Call {
    name: String,
    arguments: String,
    millis: u64,
    chars: usize,
    success: bool,
}

struct Round {
    index: usize,
    millis: u64,
    input: u64,
    cached: u64,
    output: u64,
    calls: usize,
    request_bytes: u64,
}

#[derive(Default)]
struct Scan {
    model: String,
    rounds: Vec<Round>,
    pending: BTreeMap<String, (String, String)>,
    calls: Vec<Call>,
    harness: u64,
    extra: (u64, u64, u64, u64),
    turns: usize,
    prefix: u64,
    history: (u64, u64, u64, u64),
    first_user: u64,
    footprint_kb: Option<u64>,
}

impl Scan {
    fn line(&mut self, line: Line) {
        match line {
            Line::Session {
                model: name,
                system,
                ..
            } => {
                self.model = name;
                self.prefix = system.len() as u64;
            }
            Line::Tools { bytes } => self.prefix = self.prefix.saturating_add(bytes),
            Line::User { text, start, .. } => self.user(text, start),
            Line::Assistant {
                tool_calls,
                usage,
                text,
                reasoning,
                ..
            } => self.assistant(tool_calls, usage, text, reasoning),
            Line::Tool {
                call,
                text,
                success,
                millis,
                ..
            } => self.tool(call, text, success, millis),
            Line::Charge { usage, note, .. } => self.charge(usage, note),
            Line::Receipt { text, .. } => {
                self.history.3 = self.history.3.saturating_add(text.len() as u64);
            }
            Line::Summary { text, tail } => self.summary(text, tail),
            Line::Clear => self.history = (0, 0, 0, 0),
            Line::Budget { .. }
            | Line::Workflow { .. }
            | Line::Child { .. }
            | Line::Rewrite { .. }
            | Line::Interrupted { .. } => {}
            Line::Close {
                footprint_kb: kb, ..
            } => self.footprint_kb = kb,
        }
    }

    fn user(&mut self, text: String, start: bool) {
        if start {
            self.turns += 1;
            if self.first_user == 0 {
                self.first_user = text.len() as u64;
            }
        }
        self.history.0 = self.history.0.saturating_add(text.len() as u64);
    }

    fn assistant(
        &mut self,
        tool_calls: Vec<crate::provider::ToolCall>,
        usage: crate::durable::log::Usage,
        text: String,
        reasoning: String,
    ) {
        let request_bytes = self
            .prefix
            .saturating_add(self.history.0)
            .saturating_add(self.history.1)
            .saturating_add(self.history.2)
            .saturating_add(self.history.3);
        self.history.1 = self.history.1.saturating_add(
            (text.len() + reasoning.len()) as u64
                + tool_calls
                    .iter()
                    .map(|call| call.function.arguments.len() as u64 + 40)
                    .sum::<u64>(),
        );
        for call in &tool_calls {
            self.pending.insert(
                call.id.clone(),
                (call.function.name.clone(), call.function.arguments.clone()),
            );
        }
        self.rounds.push(Round {
            index: self.rounds.len() + 1,
            millis: usage.millis,
            input: usage.input,
            cached: usage.cached,
            output: usage.output,
            calls: tool_calls.len(),
            request_bytes,
        });
    }

    fn tool(&mut self, call: String, text: String, success: bool, millis: u64) {
        self.history.2 = self.history.2.saturating_add(text.len() as u64 + 40);
        let (name, arguments) = self.pending.remove(&call).unwrap_or_default();
        self.calls.push(Call {
            name,
            arguments,
            millis,
            chars: text.chars().count(),
            success,
        });
    }

    fn charge(&mut self, usage: crate::durable::log::Usage, note: String) {
        if note == UNACCOUNTED {
            self.harness = self.harness.saturating_add(usage.millis);
            return;
        }
        self.extra.0 = self.extra.0.saturating_add(usage.millis);
        self.extra.1 = self.extra.1.saturating_add(usage.input);
        self.extra.2 = self.extra.2.saturating_add(usage.cached);
        self.extra.3 = self.extra.3.saturating_add(usage.output);
    }

    fn summary(&mut self, text: String, tail: Vec<crate::provider::Message>) {
        self.history.0 = self.first_user;
        self.history.1 = 0;
        self.history.2 = 0;
        self.history.3 = (text.len()
            + tail
                .iter()
                .map(|message| message.content.len() + 40)
                .sum::<usize>()) as u64;
    }
}

pub fn profile(path: &Path) -> Result<String> {
    let mut scan = Scan::default();
    for line in log::read(path)? {
        scan.line(line);
    }
    let view = View {
        model: &scan.model,
        turns: scan.turns,
        rounds: &scan.rounds,
        calls: &scan.calls,
        harness: scan.harness,
        extra: scan.extra,
        prefix: scan.prefix,
        history: scan.history,
        footprint_kb: scan.footprint_kb,
    };
    Ok(render(&view))
}

mod aggregate;
mod fmt;
mod health;
mod render;

use render::{View, render};

pub fn profile_all(last: Option<usize>) -> Result<String> {
    aggregate::profile_all(last)
}
