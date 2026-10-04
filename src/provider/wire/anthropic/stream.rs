use serde_json::{Value, json};

use super::event::{Change, Counted, Event, EventKind, Opened, Shape};
use super::stop_reason;
use crate::provider::message::Checked;
use crate::provider::wire::{Accumulate, Delta};
use crate::provider::{
    AssistantMessage, Partial, StopReason, ToolCall, ToolFunction, ToolKind, Usage,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Block {
    #[default]
    Other,
    Raw(Value),
    Text {
        lead: String,
        start: usize,
    },
    Thinking {
        lead: String,
        start: usize,
        signature: String,
    },
    Call {
        id: String,
        name: String,
        input: String,
    },
}

enum Kept {
    Raw(Value),
    Text {
        lead: String,
        range: std::ops::Range<usize>,
    },
    Thinking {
        lead: String,
        range: std::ops::Range<usize>,
        signature: String,
    },
    ToolUse(String),
}

impl Kept {
    fn thought(&self) -> bool {
        match self {
            Self::Thinking { .. } => true,
            Self::Raw(block) => matches!(
                block.get("type").and_then(Value::as_str),
                Some("thinking" | "redacted_thinking")
            ),
            Self::Text { .. } | Self::ToolUse(_) => false,
        }
    }

    fn value(self, text: &str, thinking: &str) -> Value {
        match self {
            Self::Raw(block) => block,
            Self::Text { lead, range } => {
                json!({"type": "text", "text": lead + &text[range]})
            }
            Self::Thinking {
                lead,
                range,
                signature,
            } => json!({
                "type": "thinking",
                "thinking": lead + &thinking[range],
                "signature": signature,
            }),
            Self::ToolUse(id) => json!({"type": "tool_use", "id": id}),
        }
    }
}

#[derive(Default)]
pub(crate) struct Stream {
    pub(crate) model: String,
    blocks: Vec<Kept>,
    block: Block,
    text: String,
    thinking: String,
    calls: Vec<ToolCall>,
    usage: Usage,
    counted: Counted,
    broken: Option<crate::error::Error>,
    stop: StopReason,
    done: bool,
}

impl Accumulate for Stream {
    fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            ..Self::default()
        }
    }

    fn on(&mut self, event: &str) -> crate::error::Result<Vec<Delta>> {
        let Some(event) = crate::provider::wire::parse::<Option<Event<'_>>>(event)? else {
            return Ok(Vec::new());
        };
        Ok(self.event(&event)?.into_iter().collect())
    }

    fn finished(&self) -> bool {
        self.done
    }

    fn finish(self) -> AssistantMessage {
        self.done()
    }

    fn spill(&mut self, partial: &mut Partial) {
        partial.text.push_str(&self.text);
        partial.reasoning.push_str(&self.thinking);
    }
}

impl Stream {
    fn event(&mut self, event: &Event<'_>) -> crate::error::Result<Option<Delta>> {
        Ok(match event.kind {
            EventKind::MessageStart => {
                self.counted = event.message.usage.unwrap_or_default();
                self.usage = self.counted.usage();
                None
            }
            EventKind::ContentBlockStart => {
                if let Some(block) = event.content_block {
                    self.block = opened(block, self.text.len(), self.thinking.len())?;
                }
                if let Block::Call { name, .. } = &self.block {
                    Some(Delta::Call(name.clone()))
                } else {
                    None
                }
            }
            EventKind::ContentBlockDelta => self.delta(&event.delta),
            EventKind::ContentBlockStop => {
                self.close();
                None
            }
            EventKind::MessageDelta => self.message_delta(event)?,
            EventKind::MessageStop => {
                self.done = true;
                None
            }
            EventKind::Error => {
                return Err(crate::provider::wire::failed(
                    None,
                    &event.error.kind,
                    &event.error.message,
                ));
            }
            EventKind::Other => None,
        })
    }

    fn message_delta(&mut self, event: &Event<'_>) -> crate::error::Result<Option<Delta>> {
        if let Some(fields) = event.usage {
            self.counted.merge(fields);
        }
        self.usage = self.counted.usage();
        let raw = event.delta.stop_reason.as_deref();
        if let Some(reason @ ("refusal" | "pause_turn")) = raw {
            let explanation = event
                .delta
                .stop_details
                .as_ref()
                .map_or("", |details| &details.explanation);
            return Err(crate::provider::wire::blocked(&if explanation.is_empty() {
                reason.to_string()
            } else {
                format!("{reason}: {explanation}")
            }));
        }
        self.stop = stop_reason(raw);
        if let Some(error) = self.broken.take()
            && self.stop != StopReason::Length
        {
            return Err(error);
        }
        Ok(None)
    }

    fn done(mut self) -> AssistantMessage {
        let mut calls = std::mem::take(&mut self.calls);
        if !calls.is_empty()
            && self.blocks.iter().any(Kept::thought)
            && let Some(call) = calls.first_mut()
        {
            let blocks = std::mem::take(&mut self.blocks)
                .into_iter()
                .map(|block| block.value(&self.text, &self.thinking))
                .collect();
            call.continuation = Some(crate::provider::Continuation::Anthropic {
                model: self.model.clone(),
                blocks,
            });
        }
        AssistantMessage {
            content: (!self.text.is_empty()).then_some(self.text),
            tool_calls: calls,
            reasoning_content: (!self.thinking.is_empty()).then_some(self.thinking),
            usage: self.usage,
            stop_reason: self.stop,
            continuation: None,
        }
    }
}

impl Stream {
    fn delta(&mut self, change: &Change<'_>) -> Option<Delta> {
        match &mut self.block {
            Block::Text { .. } => {
                let chunk = change.text.to_string();
                self.text.push_str(&chunk);
                Some(Delta::Text(chunk))
            }
            Block::Thinking { signature, .. } => {
                signature.push_str(&change.signature);
                if change.thinking.is_empty() {
                    return (!change.signature.is_empty()).then_some(Delta::Progress);
                }
                let chunk = change.thinking.to_string();
                self.thinking.push_str(&chunk);
                Some(Delta::Thinking(chunk))
            }
            Block::Call { input, .. } => {
                input.push_str(&change.partial_json);
                (!change.partial_json.is_empty()).then_some(Delta::Progress)
            }
            Block::Raw(_) | Block::Other => None,
        }
    }

    fn close(&mut self) {
        match std::mem::take(&mut self.block) {
            Block::Thinking {
                lead,
                start,
                signature,
            } => self.blocks.push(Kept::Thinking {
                lead,
                range: start..self.thinking.len(),
                signature,
            }),
            Block::Text { lead, start } if !lead.is_empty() || self.text.len() > start => {
                self.blocks.push(Kept::Text {
                    lead,
                    range: start..self.text.len(),
                });
            }
            Block::Raw(block) => self.blocks.push(Kept::Raw(block)),
            Block::Call { id, name, input } => self.call(id, name, input),
            Block::Text { .. } | Block::Other => {}
        }
    }

    fn call(&mut self, id: String, name: String, input: String) {
        let arguments = if input.is_empty() {
            "{}".to_string()
        } else {
            input
        };
        if let Err(error) = crate::provider::wire::arguments(&name, &arguments) {
            self.broken = Some(error);
            return;
        }
        self.blocks.push(Kept::ToolUse(id.clone()));
        self.calls.push(ToolCall {
            id,
            kind: ToolKind::Function,
            function: ToolFunction {
                name,
                arguments,
                checked: Checked::Valid,
            },
            continuation: None,
        });
    }

    #[cfg(test)]
    pub(crate) fn signature(&self) -> &str {
        self.blocks
            .iter()
            .rev()
            .find_map(|block| match block {
                Kept::Thinking { signature, .. } => Some(signature.as_str()),
                Kept::Raw(block) => block.get("signature").and_then(Value::as_str),
                Kept::Text { .. } | Kept::ToolUse(_) => None,
            })
            .unwrap_or("")
    }
}

fn opened(
    block: &serde_json::value::RawValue,
    text: usize,
    thinking: usize,
) -> crate::error::Result<Block> {
    let opened = crate::provider::wire::parse::<Opened<'_>>(block.get())?;
    Ok(match opened.shape {
        Shape::Text => Block::Text {
            lead: opened.text.into_owned(),
            start: text,
        },
        Shape::Thinking => Block::Thinking {
            lead: opened.thinking.into_owned(),
            start: thinking,
            signature: opened.signature.into_owned(),
        },
        Shape::ToolUse => Block::Call {
            id: opened.id.into_owned(),
            name: opened.name.into_owned(),
            input: String::new(),
        },
        Shape::Other => Block::Raw(crate::provider::wire::parse::<Value>(block.get())?),
    })
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
