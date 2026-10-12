use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::provider::{Message, Role, ToolFunction};
use crate::text::{Fnv64, plural};

const MARK: &str = "[summon control: repeated tool results.";
const CYCLE_MAX: usize = 3;

pub(super) fn nudge(history: &[Message], function: &ToolFunction, text: &str) -> Option<String> {
    let repeats = crate::limits::limits().repeat_nudge;
    if repeats < 2 {
        return None;
    }
    let keys = recent(history, key(function, text), CYCLE_MAX * repeats);
    let cycle = (1..=CYCLE_MAX).find(|&cycle| {
        let span = cycle * repeats;
        keys.len() >= span && (cycle..span).all(|index| keys[index] == keys[index - cycle])
    })?;
    Some(format!(
        "\n{MARK} The last {} returned the same {} {repeats} times in a row. Do not repeat {} unchanged: say what the result shows, then change the approach or report what blocks you. Waiting on a job that still makes progress is fine.]",
        plural(cycle, "call"),
        if cycle == 1 { "result" } else { "results" },
        if cycle == 1 { "it" } else { "them" },
    ))
}

fn nudged(text: &str) -> bool {
    text.rsplit_once('\n')
        .is_some_and(|(_, last)| last.starts_with(MARK))
}

fn recent(history: &[Message], newest: u64, limit: usize) -> Vec<u64> {
    let mut keys = vec![newest];
    let mut results: Vec<(&str, &str)> = Vec::new();
    for message in history.iter().rev() {
        match message.role {
            Role::Tool if nudged(&message.content) => break,
            Role::Tool => {
                if let Some(id) = &message.tool_call_id {
                    results.push((id, &message.content));
                }
            }
            Role::Assistant => {
                let calls: HashMap<&str, &ToolFunction> = message
                    .tool_calls
                    .iter()
                    .map(|call| (call.id.as_str(), &call.function))
                    .collect();
                for (id, text) in results.drain(..) {
                    if let Some(function) = calls.get(id) {
                        keys.push(key(function, text));
                    }
                }
                if keys.len() >= limit {
                    break;
                }
            }
            Role::User | Role::System => break,
        }
    }
    keys
}

fn key(function: &ToolFunction, text: &str) -> u64 {
    let mut hasher = Fnv64::default();
    (&function.name, &function.arguments, text).hash(&mut hasher);
    hasher.finish()
}
