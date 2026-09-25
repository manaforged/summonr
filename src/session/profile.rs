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

pub fn profile(path: &Path) -> Result<String> {
    let mut model = String::new();
    let mut rounds: Vec<Round> = Vec::new();
    let mut pending: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut calls: Vec<Call> = Vec::new();
    let mut harness = 0_u64;
    let mut extra = (0_u64, 0_u64, 0_u64, 0_u64);
    let mut turns = 0_usize;
    let mut prefix = 0_u64;
    let mut history = (0_u64, 0_u64, 0_u64, 0_u64);
    let mut first_user = 0_u64;
    let mut footprint_kb = None;
    for line in log::read(path)? {
        match line {
            Line::Session {
                model: name,
                system,
                ..
            } => {
                model = name;
                prefix = system.len() as u64;
            }
            Line::Tools { bytes } => prefix = prefix.saturating_add(bytes),
            Line::User {
                start: true, text, ..
            } => {
                turns += 1;
                history.0 = history.0.saturating_add(text.len() as u64);
                if first_user == 0 {
                    first_user = text.len() as u64;
                }
            }
            Line::User { text, .. } => {
                history.0 = history.0.saturating_add(text.len() as u64);
            }
            Line::Assistant {
                tool_calls,
                usage,
                text,
                reasoning,
                ..
            } => {
                let request_bytes = prefix
                    .saturating_add(history.0)
                    .saturating_add(history.1)
                    .saturating_add(history.2)
                    .saturating_add(history.3);
                history.1 = history.1.saturating_add(
                    (text.len() + reasoning.len()) as u64
                        + tool_calls
                            .iter()
                            .map(|call| call.function.arguments.len() as u64 + 40)
                            .sum::<u64>(),
                );
                for call in &tool_calls {
                    pending.insert(
                        call.id.clone(),
                        (call.function.name.clone(), call.function.arguments.clone()),
                    );
                }
                rounds.push(Round {
                    index: rounds.len() + 1,
                    millis: usage.millis,
                    input: usage.input,
                    cached: usage.cached,
                    output: usage.output,
                    calls: tool_calls.len(),
                    request_bytes,
                });
            }
            Line::Tool {
                call,
                text,
                success,
                millis,
                ..
            } => {
                history.2 = history.2.saturating_add(text.len() as u64 + 40);
                let (name, arguments) = pending.remove(&call).unwrap_or_default();
                calls.push(Call {
                    name,
                    arguments,
                    millis,
                    chars: text.chars().count(),
                    success,
                });
            }
            Line::Charge { usage, note, .. } if note == UNACCOUNTED => {
                harness = harness.saturating_add(usage.millis);
            }
            Line::Charge { usage, .. } => {
                extra.0 = extra.0.saturating_add(usage.millis);
                extra.1 = extra.1.saturating_add(usage.input);
                extra.2 = extra.2.saturating_add(usage.cached);
                extra.3 = extra.3.saturating_add(usage.output);
            }
            Line::Receipt { text, .. } => {
                history.3 = history.3.saturating_add(text.len() as u64);
            }
            Line::Summary { text, tail } => {
                history.0 = first_user;
                history.1 = 0;
                history.2 = 0;
                history.3 = (text.len()
                    + tail
                        .iter()
                        .map(|message| message.content.len() + 40)
                        .sum::<usize>()) as u64;
            }
            Line::Clear => history = (0, 0, 0, 0),
            Line::Budget { .. }
            | Line::Workflow { .. }
            | Line::Child { .. }
            | Line::Rewrite { .. }
            | Line::Interrupted { .. } => {}
            Line::Close {
                footprint_kb: kb, ..
            } => footprint_kb = kb,
        }
    }
    let view = View {
        model: &model,
        turns,
        rounds: &rounds,
        calls: &calls,
        harness,
        extra,
        prefix,
        history,
        footprint_kb,
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
