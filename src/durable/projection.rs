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

fn said(text: &str, reasoning: &str) -> Vec<Event> {
    let mut events = Vec::new();
    if !reasoning.is_empty() {
        events.push(Event::Reasoning {
            text: reasoning.to_string(),
        });
    }
    if !text.is_empty() {
        events.push(Event::Assistant {
            text: text.to_string(),
        });
    }
    events
}

impl Line {
    pub(crate) fn events(&self) -> Vec<Event> {
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
                let mut events = said(text, reasoning);
                if let Some(event) = usage_event(usage, prefix_hash.clone(), *cost) {
                    events.push(event);
                }
                events.extend(tool_calls.iter().map(|call| Event::ToolCall {
                    id: call.id.clone(),
                    name: call.function.name.clone(),
                    arguments: call.function.arguments.clone(),
                }));
                events
            }
            Line::Interrupted { text, reasoning } => said(text, reasoning),
            Line::Tool {
                call,
                text,
                success,
                mutated,
                millis,
            } => vec![Event::ToolResult {
                id: call.clone(),
                success: *success,
                mutated: *mutated,
                output: text.clone(),
                millis: Some(*millis),
            }],
            Line::Charge {
                usage,
                reported: true,
                prefix_hash,
                cost,
                ..
            } => usage_event(usage, prefix_hash.clone(), *cost)
                .into_iter()
                .collect(),
            Line::Receipt {
                receipt: Some(receipt),
                ..
            } => vec![Event::Receipt {
                receipt: receipt.clone(),
            }],
            Line::Close {
                reason: Some(reason),
                ..
            } => vec![Event::Aborted {
                reason: reason.clone(),
            }],
            _ => Vec::new(),
        }
    }
}
