use std::collections::BTreeMap;
use std::path::Path;

use crate::durable::log::{self, Line, Usage};
use summonr_core::error::Result;

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
    usage: Usage,
    calls: usize,
    request_bytes: u64,
}

#[derive(Clone, Copy, Default)]
struct History {
    user: u64,
    assistant: u64,
    tool: u64,
    receipt: u64,
}

impl History {
    fn bytes(self) -> u64 {
        self.user
            .saturating_add(self.assistant)
            .saturating_add(self.tool)
            .saturating_add(self.receipt)
    }
}

#[derive(Default)]
struct Scan {
    model: String,
    rounds: Vec<Round>,
    pending: BTreeMap<String, (String, String)>,
    calls: Vec<Call>,
    harness: u64,
    extra: Usage,
    turns: usize,
    prefix: u64,
    history: History,
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
                model,
                ..
            } => {
                if !model.is_empty() {
                    self.model = model;
                }
                self.assistant(tool_calls, usage, text, reasoning);
            }
            Line::Tool {
                call,
                text,
                success,
                millis,
                ..
            } => self.tool(call, text, success, millis),
            Line::Charge { usage, note, .. } => self.charge(usage, note),
            Line::Receipt { text, .. } => {
                self.history.receipt = self.history.receipt.saturating_add(text.len() as u64);
            }
            Line::Summary { text, tail } => self.summary(text, tail),
            Line::Clear => self.history = History::default(),
            Line::Budget { .. }
            | Line::Work { .. }
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
        self.history.user = self.history.user.saturating_add(text.len() as u64);
    }

    fn assistant(
        &mut self,
        tool_calls: Vec<summonr_types::ToolCall>,
        usage: Usage,
        text: String,
        reasoning: String,
    ) {
        let request_bytes = self.prefix.saturating_add(self.history.bytes());
        self.history.assistant = self.history.assistant.saturating_add(
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
            usage,
            calls: tool_calls.len(),
            request_bytes,
        });
    }

    fn tool(&mut self, call: String, text: String, success: bool, millis: u64) {
        self.history.tool = self.history.tool.saturating_add(text.len() as u64 + 40);
        let (name, arguments) = self.pending.remove(&call).unwrap_or_default();
        self.calls.push(Call {
            name,
            arguments,
            millis,
            chars: text.chars().count(),
            success,
        });
    }

    fn charge(&mut self, usage: Usage, note: String) {
        if note == UNACCOUNTED {
            self.harness = self.harness.saturating_add(usage.millis);
            return;
        }
        self.extra.add(usage);
    }

    fn summary(&mut self, text: String, tail: Vec<summonr_types::Message>) {
        self.history.user = self.first_user;
        self.history.assistant = 0;
        self.history.tool = 0;
        self.history.receipt = (text.len()
            + tail
                .iter()
                .map(|message| message.content.len() + 40)
                .sum::<usize>()) as u64;
    }
}

#[derive(Clone, Copy)]
pub enum Scope<'a> {
    Session(&'a Path),
    Recent(Option<usize>),
}

pub fn profile(scope: Scope<'_>) -> Result<String> {
    match scope {
        Scope::Session(path) => session(path),
        Scope::Recent(last) => aggregate::profile_all(last),
    }
}

fn session(path: &Path) -> Result<String> {
    let mut scan = Scan::default();
    log::records(path, 0).try_for_each(|line| line.map(|line| scan.line(line)))?;
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
