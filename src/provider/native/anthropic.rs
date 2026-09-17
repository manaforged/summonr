use genai::chat::RawFrameRef;
use serde::Deserialize;
use serde_json::Value;

use crate::error::{Error, ProviderFailureKind, Result};
use crate::provider::{AssistantMessage, Continuation};

const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Default)]
pub(super) struct Capture {
    blocks: Vec<Value>,
    open: Option<usize>,
    input: String,
    bytes: usize,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Event {
    ContentBlockStart {
        index: usize,
        content_block: Value,
    },
    ContentBlockDelta {
        index: usize,
        delta: Value,
    },
    ContentBlockStop {
        index: usize,
    },
    #[serde(other)]
    Other,
}

impl Capture {
    pub(super) fn push(&mut self, frame: RawFrameRef<'_>) -> Result<()> {
        self.bytes = self.bytes.saturating_add(frame.data.len());
        if self.bytes > MAX_BYTES {
            return Err(malformed("Anthropic continuation exceeds 16 MiB"));
        }
        let event: Event = serde_json::from_str(frame.data)
            .map_err(|error| malformed(&format!("invalid Anthropic content event: {error}")))?;
        match event {
            Event::ContentBlockStart {
                index,
                content_block,
            } => {
                if self.open.is_some() || index != self.blocks.len() {
                    return Err(malformed("Anthropic content blocks are out of order"));
                }
                if content_block.get("type").and_then(Value::as_str).is_none() {
                    return Err(malformed("Anthropic content block has no type"));
                }
                self.blocks.push(content_block);
                self.open = Some(index);
            }
            Event::ContentBlockDelta { index, delta } => {
                if self.open != Some(index) {
                    return Err(malformed("Anthropic delta has no open content block"));
                }
                match delta.get("type").and_then(Value::as_str) {
                    Some("input_json_delta") => {
                        if self.blocks[index]["type"] != "tool_use" {
                            return Err(malformed("Anthropic tool input has no tool block"));
                        }
                        self.input.push_str(field(&delta, "partial_json")?);
                    }
                    Some("text_delta") => self.append(index, "text", field(&delta, "text")?)?,
                    Some("thinking_delta") => {
                        self.append(index, "thinking", field(&delta, "thinking")?)?
                    }
                    Some("signature_delta") => {
                        self.append(index, "signature", field(&delta, "signature")?)?
                    }
                    Some("citations_delta") => {
                        if self.blocks[index]["type"] != "text" {
                            return Err(malformed("Anthropic citation has no text block"));
                        }
                        let citation = delta
                            .get("citation")
                            .ok_or_else(|| malformed("Anthropic citation delta has no citation"))?;
                        let block = &mut self.blocks[index];
                        let citations = block
                            .as_object_mut()
                            .ok_or_else(|| malformed("invalid Anthropic content block"))?
                            .entry("citations")
                            .or_insert_with(|| Value::Array(Vec::new()));
                        citations
                            .as_array_mut()
                            .ok_or_else(|| malformed("invalid Anthropic citations"))?
                            .push(citation.clone());
                    }
                    _ => return Err(malformed("unsupported Anthropic content delta")),
                }
            }
            Event::ContentBlockStop { index } => {
                if self.open != Some(index) {
                    return Err(malformed("Anthropic stop has no open content block"));
                }
                if !self.input.is_empty() {
                    self.blocks[index]["input"] =
                        serde_json::from_str(&self.input).map_err(|error| {
                            malformed(&format!("invalid Anthropic tool input: {error}"))
                        })?;
                    self.input.clear();
                }
                self.open = None;
            }
            Event::Other => {}
        }
        Ok(())
    }

    fn append(&mut self, index: usize, key: &str, text: &str) -> Result<()> {
        let value = self.blocks[index]
            .get_mut(key)
            .ok_or_else(|| malformed("Anthropic delta field is missing from its block"))?;
        let Value::String(value) = value else {
            return Err(malformed("Anthropic delta field is not text"));
        };
        value.push_str(text);
        Ok(())
    }

    pub(super) fn finish(self) -> Result<Vec<Value>> {
        if self.open.is_some() {
            return Err(malformed(
                "Anthropic stream ended with an open content block",
            ));
        }
        Ok(self.blocks)
    }
}

pub(super) fn blocks(body: Option<&Value>) -> Result<Vec<Value>> {
    let blocks = body
        .and_then(|body| body.get("content"))
        .and_then(Value::as_array)
        .ok_or_else(|| malformed("Anthropic response has no content blocks"))?;
    let bytes = serde_json::to_vec(blocks)?;
    if bytes.len() > MAX_BYTES {
        return Err(malformed("Anthropic continuation exceeds 16 MiB"));
    }
    Ok(blocks.clone())
}

pub(super) fn retain(message: &mut AssistantMessage, model: &str, blocks: Option<Vec<Value>>) {
    if let Some(blocks) = blocks {
        for call in &mut message.tool_calls {
            call.continuation = None;
        }
        if let Some(call) = message.tool_calls.first_mut() {
            call.continuation = Some(Continuation::Anthropic {
                model: model.to_string(),
                blocks,
            });
        }
    }
}

fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| malformed("Anthropic delta is missing its text field"))
}

fn malformed(message: &str) -> Error {
    Error::provider(ProviderFailureKind::Malformed, message)
}
