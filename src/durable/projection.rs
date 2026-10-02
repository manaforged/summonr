use super::log::{Line, Usage};
use crate::tools::Event;

fn usage_event(usage: &Usage, prefix_hash: String, cost: Option<f64>) -> Option<Event> {
    if usage.input == 0 && usage.output == 0 {
        return None;
    }
    Some(Event::Usage {
        prompt_tokens: u32::try_from(usage.input).unwrap_or(u32::MAX),
        completion_tokens: u32::try_from(usage.output).unwrap_or(u32::MAX),
        reasoning_tokens: u32::try_from(usage.reasoning).unwrap_or(u32::MAX),
        cached_tokens: u32::try_from(usage.cached).unwrap_or(u32::MAX),
        millis: Some(usage.millis),
        prefix_hash,
        cost,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Speech {
    Include,
    Skip,
}

fn said(text: String, reasoning: String, speech: Speech) -> Vec<Event> {
    let mut events = Vec::new();
    if speech == Speech::Skip {
        return events;
    }
    if !reasoning.is_empty() {
        events.push(Event::Reasoning { text: reasoning });
    }
    if !text.is_empty() {
        events.push(Event::Assistant { text });
    }
    events
}

impl Line {
    pub(crate) fn events(&self) -> Vec<Event> {
        self.clone().into_events(Speech::Include)
    }

    pub(crate) fn into_events(self, speech: Speech) -> Vec<Event> {
        match self {
            Line::Assistant {
                text,
                reasoning,
                tool_calls,
                usage,
                prefix_hash,
                cost,
                ..
            } => {
                let mut events = said(text, reasoning, speech);
                events.extend(usage_event(&usage, prefix_hash, cost));
                events.extend(tool_calls.into_iter().map(|call| Event::ToolCall {
                    id: call.id,
                    name: call.function.name,
                    arguments: call.function.arguments,
                }));
                events
            }
            Line::Interrupted { text, reasoning } => said(text, reasoning, speech),
            Line::Tool {
                call,
                text,
                success,
                mutated,
                millis,
            } => vec![Event::ToolResult {
                id: call,
                success,
                mutated,
                output: text,
                millis: Some(millis),
            }],
            Line::Charge {
                usage,
                reported: true,
                prefix_hash,
                cost,
                ..
            } => usage_event(&usage, prefix_hash, cost).into_iter().collect(),
            Line::Receipt {
                receipt: Some(receipt),
                ..
            } => vec![Event::Receipt { receipt }],
            Line::Close {
                reason: Some(reason),
                ..
            } => vec![Event::Aborted { reason }],
            _ => Vec::new(),
        }
    }
}
