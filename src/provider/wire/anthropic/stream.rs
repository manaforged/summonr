use std::ops::Range;

use serde_json::{Value, json};

use super::event::{Change, Counted, Event, EventKind, Opened, Shape};
use crate::provider::stream::append_arguments;
use crate::provider::wire::reply::ReplyBuilder;
use crate::provider::wire::{Accumulate, Delta};
use crate::provider::{AssistantMessage, StopReason, ToolCall, ToolFunction, ToolKind};
use summonr_types::Checked;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum ContentBlock {
    #[default]
    Other,
    Raw(Value),
    Text {
        lead: String,
        range: Range<usize>,
    },
    Thinking {
        lead: String,
        range: Range<usize>,
        signature: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: String,
    },
}

impl ContentBlock {
    fn thought(&self) -> bool {
        match self {
            Self::Thinking { .. } => true,
            Self::Raw(block) => matches!(
                block.get("type").and_then(Value::as_str),
                Some("thinking" | "redacted_thinking")
            ),
            Self::Text { .. } | Self::ToolUse { .. } | Self::Other => false,
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
            Self::ToolUse { id, .. } => json!({"type": "tool_use", "id": id}),
            Self::Other => Value::Null,
        }
    }
}

#[derive(Default)]
pub(crate) struct Stream {
    pub(crate) model: String,
    blocks: Vec<ContentBlock>,
    block: ContentBlock,
    reply: ReplyBuilder,
    counted: Counted,
    broken: Option<summonr_core::error::Error>,
}

impl Accumulate for Stream {
    fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            ..Self::default()
        }
    }

    fn on(&mut self, event: &str) -> summonr_core::error::Result<Vec<Delta>> {
        let Some(event) = crate::provider::wire::parse::<Option<Event<'_>>>(event)? else {
            return Ok(Vec::new());
        };
        Ok(self.event(&event)?.into_iter().collect())
    }

    fn reply(&self) -> &ReplyBuilder {
        &self.reply
    }

    fn finish(mut self) -> AssistantMessage {
        let reply = &mut self.reply;
        if !reply.calls.is_empty()
            && self.blocks.iter().any(ContentBlock::thought)
            && let Some(call) = reply.calls.first_mut()
        {
            let blocks = std::mem::take(&mut self.blocks)
                .into_iter()
                .map(|block| block.value(&reply.text, &reply.reasoning))
                .collect();
            call.continuation = Some(crate::provider::Continuation::Anthropic {
                model: self.model.clone(),
                blocks,
            });
        }
        self.reply.finish(None)
    }
}

impl Stream {
    fn event(&mut self, event: &Event<'_>) -> summonr_core::error::Result<Option<Delta>> {
        Ok(match event.kind {
            EventKind::MessageStart => {
                self.counted = event.message.usage.unwrap_or_default();
                self.reply.usage = self.counted.usage();
                None
            }
            EventKind::ContentBlockStart => {
                if let Some(block) = event.content_block {
                    self.block = opened(block, self.reply.text.len(), self.reply.reasoning.len())?;
                }
                if let ContentBlock::ToolUse { name, .. } = &self.block {
                    Some(Delta::Call(name.clone()))
                } else {
                    None
                }
            }
            EventKind::ContentBlockDelta => self.delta(&event.delta)?,
            EventKind::ContentBlockStop => {
                self.close();
                None
            }
            EventKind::MessageDelta => self.message_delta(event)?,
            EventKind::MessageStop => {
                self.reply.finished = true;
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

    fn message_delta(&mut self, event: &Event<'_>) -> summonr_core::error::Result<Option<Delta>> {
        if let Some(fields) = event.usage {
            self.counted.merge(fields);
        }
        self.reply.usage = self.counted.usage();
        let explanation = event
            .delta
            .stop_details
            .as_ref()
            .map_or("", |details| &details.explanation);
        let raw = event.delta.stop_reason.as_deref().unwrap_or_default();
        if let Some(stop) =
            crate::provider::client::ending(crate::endpoint::Wire::Anthropic, raw, explanation)?
        {
            self.reply.stop = stop;
        }
        if let Some(error) = self.broken.take()
            && self.reply.stop != StopReason::Length
        {
            return Err(error);
        }
        Ok(None)
    }

    fn delta(&mut self, change: &Change<'_>) -> summonr_core::error::Result<Option<Delta>> {
        match &mut self.block {
            ContentBlock::Text { .. } => {
                let chunk = change.text.to_string();
                self.reply.text.push_str(&chunk);
                Ok(Some(Delta::Text(chunk)))
            }
            ContentBlock::Thinking { signature, .. } => {
                signature.push_str(&change.signature);
                if change.thinking.is_empty() {
                    return Ok((!change.signature.is_empty()).then_some(Delta::Progress));
                }
                let chunk = change.thinking.to_string();
                self.reply.reasoning.push_str(&chunk);
                Ok(Some(Delta::Thinking(chunk)))
            }
            ContentBlock::ToolUse { input, .. } => {
                append_arguments(input, &change.partial_json)?;
                Ok((!change.partial_json.is_empty()).then_some(Delta::Progress))
            }
            ContentBlock::Raw(_) | ContentBlock::Other => Ok(None),
        }
    }

    fn close(&mut self) {
        match std::mem::take(&mut self.block) {
            ContentBlock::Thinking {
                lead,
                range,
                signature,
            } => self.blocks.push(ContentBlock::Thinking {
                lead,
                range: range.start..self.reply.reasoning.len(),
                signature,
            }),
            ContentBlock::Text { lead, range }
                if !lead.is_empty() || self.reply.text.len() > range.start =>
            {
                self.blocks.push(ContentBlock::Text {
                    lead,
                    range: range.start..self.reply.text.len(),
                });
            }
            ContentBlock::ToolUse { id, name, input } => self.call(id, name, input),
            block @ ContentBlock::Raw(_) => self.blocks.push(block),
            ContentBlock::Text { .. } | ContentBlock::Other => {}
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
        self.blocks.push(ContentBlock::ToolUse {
            id: id.clone(),
            name: String::new(),
            input: String::new(),
        });
        self.reply.calls.push(ToolCall {
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
                ContentBlock::Thinking { signature, .. } => Some(signature.as_str()),
                ContentBlock::Raw(block) => block.get("signature").and_then(Value::as_str),
                ContentBlock::Text { .. } | ContentBlock::ToolUse { .. } | ContentBlock::Other => {
                    None
                }
            })
            .unwrap_or("")
    }
}

fn opened(
    block: &serde_json::value::RawValue,
    text: usize,
    thinking: usize,
) -> summonr_core::error::Result<ContentBlock> {
    let opened = crate::provider::wire::parse::<Opened<'_>>(block.get())?;
    Ok(match opened.shape {
        Shape::Text => ContentBlock::Text {
            lead: opened.text.into_owned(),
            range: text..text,
        },
        Shape::Thinking => ContentBlock::Thinking {
            lead: opened.thinking.into_owned(),
            range: thinking..thinking,
            signature: opened.signature.into_owned(),
        },
        Shape::ToolUse => ContentBlock::ToolUse {
            id: opened.id.into_owned(),
            name: opened.name.into_owned(),
            input: String::new(),
        },
        Shape::Other => ContentBlock::Raw(crate::provider::wire::parse::<Value>(block.get())?),
    })
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
