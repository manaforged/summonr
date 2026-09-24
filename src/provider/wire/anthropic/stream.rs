use serde_json::{Value, json};

use super::{stop_reason, usage};
use crate::provider::wire::{Accumulate, Delta};
use crate::provider::{AssistantMessage, StopReason, ToolCall, ToolFunction, ToolKind, Usage};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Block {
    #[default]
    Other,
    Raw(Value),
    Text(String),
    Thinking {
        text: String,
        signature: String,
    },
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
    calls: Vec<ToolCall>,
    usage: Usage,
    counted: Value,
    broken: Option<crate::error::Error>,
    stop: StopReason,
    done: bool,
}

impl Accumulate for Stream {
    fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            counted: json!({}),
            ..Self::default()
        }
    }

    fn on(&mut self, event: &str) -> crate::error::Result<Vec<Delta>> {
        let event = crate::provider::wire::parse::<Value>(event)?;
        Ok(self.event(&event)?.into_iter().collect())
    }

    fn finished(&self) -> bool {
        self.done
    }

    fn finish(self) -> AssistantMessage {
        self.done()
    }
}

impl Stream {
    fn event(&mut self, event: &Value) -> crate::error::Result<Option<Delta>> {
        let Some(kind) = event.get("type").and_then(Value::as_str) else {
            return Ok(None);
        };
        Ok(match kind {
            "message_start" => {
                self.counted = event
                    .pointer("/message/usage")
                    .filter(|counted| counted.is_object())
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                self.usage = usage(Some(&self.counted));
                None
            }
            "content_block_start" => {
                if let Some(block) = event.pointer("/content_block") {
                    self.block = opened(block);
                }
                if let Block::Call { name, .. } = &self.block {
                    Some(Delta::Call(name.clone()))
                } else {
                    None
                }
            }
            "content_block_delta" => self.delta(event),
            "content_block_stop" => {
                self.close();
                None
            }
            "message_delta" => {
                if let (Some(Value::Object(fields)), Value::Object(counted)) =
                    (event.get("usage"), &mut self.counted)
                {
                    counted.extend(fields.clone());
                }
                self.usage = usage(Some(&self.counted));
                let raw = event.pointer("/delta/stop_reason").and_then(Value::as_str);
                if let Some(reason @ ("refusal" | "pause_turn")) = raw {
                    let explanation = text_at(event, "/delta/stop_details/explanation");
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
                None
            }
            "message_stop" => {
                self.done = true;
                None
            }
            "error" => {
                return Err(crate::provider::wire::failed(
                    None,
                    text_at(event, "/error/type"),
                    text_at(event, "/error/message"),
                ));
            }
            _ => None,
        })
    }

    fn done(mut self) -> AssistantMessage {
        let mut calls = std::mem::take(&mut self.calls);
        if !calls.is_empty()
            && self.blocks.iter().any(|block| {
                matches!(
                    block.get("type").and_then(Value::as_str),
                    Some("thinking" | "redacted_thinking")
                )
            })
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
            continuation: None,
        }
    }
}

impl Stream {
    fn delta(&mut self, event: &Value) -> Option<Delta> {
        match &mut self.block {
            Block::Text(text) => {
                let chunk = text_at(event, "/delta/text").to_string();
                text.push_str(&chunk);
                self.text.push_str(&chunk);
                Some(Delta::Text(chunk))
            }
            Block::Thinking { text, signature } => {
                signature.push_str(text_at(event, "/delta/signature"));
                let chunk = text_at(event, "/delta/thinking").to_string();
                if chunk.is_empty() {
                    return None;
                }
                text.push_str(&chunk);
                self.thinking.push_str(&chunk);
                Some(Delta::Thinking(chunk))
            }
            Block::Call { input, .. } => {
                input.push_str(text_at(event, "/delta/partial_json"));
                None
            }
            Block::Raw(_) | Block::Other => None,
        }
    }

    fn close(&mut self) {
        match std::mem::take(&mut self.block) {
            Block::Thinking { text, signature } => self.blocks.push(json!({
                "type": "thinking",
                "thinking": text,
                "signature": signature,
            })),
            Block::Text(text) if !text.is_empty() => {
                self.blocks.push(json!({"type": "text", "text": text}));
            }
            Block::Raw(block) => self.blocks.push(block),
            Block::Call { id, name, input } => self.call(id, name, input),
            Block::Text(_) | Block::Other => {}
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
        self.blocks.push(json!({"type": "tool_use", "id": id}));
        self.calls.push(ToolCall {
            id,
            kind: ToolKind::Function,
            function: ToolFunction { name, arguments },
            continuation: None,
        });
    }

    #[cfg(test)]
    pub(crate) fn signature(&self) -> &str {
        self.blocks
            .iter()
            .rev()
            .find_map(|block| block.get("signature").and_then(Value::as_str))
            .unwrap_or("")
    }
}

fn opened(block: &Value) -> Block {
    match block.get("type").and_then(Value::as_str) {
        Some("text") => Block::Text(text_at(block, "/text").to_string()),
        Some("thinking") => Block::Thinking {
            text: text_at(block, "/thinking").to_string(),
            signature: text_at(block, "/signature").to_string(),
        },
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
        _ => Block::Raw(block.clone()),
    }
}

fn text_at<'a>(value: &'a Value, path: &str) -> &'a str {
    value.pointer(path).and_then(Value::as_str).unwrap_or("")
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
