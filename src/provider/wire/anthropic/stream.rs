use serde_json::{Value, json};

use super::{stop_reason, usage};
use crate::provider::wire::{Accumulate, Delta};
use crate::provider::{AssistantMessage, StopReason, ToolCall, ToolFunction, ToolKind, Usage};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Block {
    #[default]
    Other,
    Text,
    Thinking,
    Call {
        id: String,
        name: String,
        input: String,
    },
}

#[derive(Default)]
pub(crate) struct Stream {
    pub(crate) model: String,
    blocks: Vec<Value>,
    block: Block,
    text: String,
    thinking: String,
    signature: String,
    calls: Vec<ToolCall>,
    usage: Usage,
    stop: StopReason,
}

impl Accumulate for Stream {
    fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            ..Self::default()
        }
    }

    fn on(&mut self, event: &str) -> crate::error::Result<Vec<Delta>> {
        let event = crate::provider::wire::parse::<Value>(event)?;
        Ok(self.event(&event).into_iter().collect())
    }

    fn finish(mut self) -> AssistantMessage {
        self.close();
        self.done()
    }
}

impl Stream {
    fn event(&mut self, event: &Value) -> Option<Delta> {
        match event.get("type").and_then(Value::as_str)? {
            "message_start" => {
                self.usage = usage(event.pointer("/message/usage"));
                None
            }
            "content_block_start" => {
                self.block = opened(event.pointer("/content_block")?);
                if let Block::Thinking = self.block {
                    self.signature
                        .push_str(text_at(event, "/content_block/signature"));
                }
                None
            }
            "content_block_delta" => self.delta(event),
            "content_block_stop" => {
                self.close();
                None
            }
            "message_delta" => {
                let counted = usage(event.get("usage"));
                self.usage.completion_tokens = counted.completion_tokens;
                self.stop =
                    stop_reason(event.pointer("/delta/stop_reason").and_then(Value::as_str));
                None
            }
            _ => None,
        }
    }

    fn done(mut self) -> AssistantMessage {
        let mut calls = std::mem::take(&mut self.calls);
        if !calls.is_empty()
            && !self.signature.is_empty()
            && let Some(call) = calls.first_mut()
        {
            call.continuation = Some(crate::provider::Continuation::Anthropic {
                model: self.model.clone(),
                blocks: std::mem::take(&mut self.blocks),
            });
        }
        AssistantMessage {
            content: (!self.text.is_empty()).then_some(self.text),
            tool_calls: calls,
            reasoning_content: (!self.thinking.is_empty()).then_some(self.thinking),
            usage: self.usage,
            stop_reason: self.stop,
        }
    }
}

impl Stream {
    fn delta(&mut self, event: &Value) -> Option<Delta> {
        match &mut self.block {
            Block::Text => {
                let chunk = text_at(event, "/delta/text").to_string();
                self.text.push_str(&chunk);
                Some(Delta::Text(chunk))
            }
            Block::Thinking => {
                self.signature.push_str(text_at(event, "/delta/signature"));
                let chunk = text_at(event, "/delta/thinking").to_string();
                if chunk.is_empty() {
                    return None;
                }
                self.thinking.push_str(&chunk);
                Some(Delta::Thinking(chunk))
            }
            Block::Call { input, .. } => {
                input.push_str(text_at(event, "/delta/partial_json"));
                None
            }
            Block::Other => None,
        }
    }

    fn close(&mut self) {
        match &self.block {
            Block::Thinking if !self.thinking.is_empty() => self.blocks.push(json!({
                "type": "thinking",
                "thinking": self.thinking,
                "signature": self.signature,
            })),
            Block::Text if !self.text.is_empty() => {
                self.blocks.push(json!({"type": "text", "text": self.text}));
            }
            _ => {}
        }
        if let Block::Call { id, name, input } = std::mem::take(&mut self.block) {
            self.blocks.push(json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": crate::provider::wire::arguments(if input.is_empty() { "{}" } else { &input }),
            }));
            self.calls.push(ToolCall {
                id,
                kind: ToolKind::Function,
                function: ToolFunction {
                    name,
                    arguments: if input.is_empty() {
                        "{}".to_string()
                    } else {
                        input
                    },
                },
                continuation: None,
            });
        }
        self.block = Block::Other;
    }

    pub(crate) fn signature(&self) -> &str {
        &self.signature
    }
}

fn opened(block: &Value) -> Block {
    match block.get("type").and_then(Value::as_str) {
        Some("text") => Block::Text,
        Some("thinking") => Block::Thinking,
        Some("tool_use") => Block::Call {
            id: block
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            name: block
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            input: String::new(),
        },
        _ => Block::Other,
    }
}

fn text_at<'a>(value: &'a Value, path: &str) -> &'a str {
    value.pointer(path).and_then(Value::as_str).unwrap_or("")
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
