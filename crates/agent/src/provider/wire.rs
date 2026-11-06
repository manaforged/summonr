use serde_json::Value;
use serde_json::value::RawValue;

use crate::provider::{AssistantMessage, CompletionRequest, Reasoning};
use summonr_core::error::{Error, ProviderFailureKind, Result};

pub(crate) mod anthropic;
pub(crate) mod completions;
pub(crate) mod decisions;
pub(crate) mod gemini;
pub(crate) mod reply;
pub(crate) mod responses;
mod spec;
pub(crate) mod traits;

pub(crate) use spec::{Codec, ImageBudget, codec, spec};

pub(crate) fn tools(
    call: &CompletionRequest<'_>,
    schema: &str,
    extra: &[(&str, Value)],
) -> Vec<Value> {
    call.tools
        .iter()
        .map(|tool| {
            let mut entry = serde_json::json!({
                "name": tool.function.name,
                "description": tool.function.description,
                (schema): tool.function.parameters,
            });
            for (key, value) in extra {
                entry[*key] = value.clone();
            }
            entry
        })
        .collect()
}

pub(crate) fn arguments<'a>(name: &str, raw: &'a str) -> Result<&'a RawValue> {
    serde_json::from_str(raw).map_err(|error| {
        Error::provider(
            ProviderFailureKind::MalformedToolCall,
            format!("{name}: invalid tool arguments: {error}"),
        )
    })
}

pub(crate) fn checked(function: &crate::provider::ToolFunction) -> Result<()> {
    if function.valid() {
        return Ok(());
    }
    arguments(&function.name, &function.arguments).map(drop)
}

pub(crate) fn failed(status: Option<u16>, kind: &str, message: &str) -> Error {
    let code = status.unwrap_or_else(|| super::client::status_code(kind));
    let mut error = super::client::response_failure(code, &format!("{kind}: {message}"), None);
    if let Error::Provider(failure) = &mut error {
        failure.status = status;
    }
    error
}

pub(crate) fn blocked(reason: &str) -> Error {
    Error::provider(
        ProviderFailureKind::Http,
        format!("native response was blocked by the provider: {reason}"),
    )
}

pub enum Delta {
    Text(String),
    Thinking(String),
    Call(String),
    Progress,
}

pub(crate) const MOST_PENDING: usize = 64 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct Frames {
    buffer: Vec<u8>,
    offset: usize,
    scanned: usize,
    ended: bool,
}

impl Frames {
    pub(crate) fn push(&mut self, bytes: &[u8], sentinel: Option<&str>) -> Result<Vec<&str>> {
        if self.offset > 0 {
            self.buffer.drain(..self.offset);
            self.scanned = self.scanned.saturating_sub(self.offset);
            self.offset = 0;
        }
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > MOST_PENDING {
            return Err(Error::provider(
                ProviderFailureKind::Malformed,
                format!("{MOST_PENDING} bytes arrived with no frame boundary"),
            ));
        }
        let Self {
            buffer,
            offset,
            scanned,
            ended,
        } = self;
        let buffer: &[u8] = buffer;
        let mut frames = Vec::new();
        while let Some((end, width)) = boundary(buffer, (*scanned).max(*offset)) {
            let event = &buffer[*offset..end];
            *offset = end + width;
            *scanned = *offset;
            let text = std::str::from_utf8(event).map_err(|error| {
                Error::provider(
                    ProviderFailureKind::Malformed,
                    format!("invalid SSE bytes: {error}"),
                )
            })?;
            for line in text.lines() {
                let Some(data) = line.strip_prefix("data:").map(str::trim) else {
                    continue;
                };
                if data.is_empty() {
                    continue;
                }
                if sentinel.is_some_and(|mark| data == mark) {
                    *ended = true;
                    return Ok(frames);
                }
                frames.push(data);
            }
        }
        *scanned = buffer.len().saturating_sub(3).max(*offset);
        Ok(frames)
    }

    pub(crate) fn ended(&self) -> bool {
        self.ended
    }
}

pub(crate) fn parse<'a, T: serde::Deserialize<'a>>(data: &'a str) -> Result<T> {
    serde_json::from_str(data).map_err(|error| {
        Error::provider(
            ProviderFailureKind::Malformed,
            format!("invalid stream frame: {error}"),
        )
    })
}

pub(crate) fn boundary(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut at = from.min(bytes.len());
    while let Some(found) = bytes[at..].iter().position(|byte| *byte == b'\n') {
        let newline = at + found;
        let next = bytes.get(newline + 1).copied();
        if next == Some(b'\n') {
            return Some((newline, 2));
        }
        if newline > from
            && bytes[newline - 1] == b'\r'
            && next == Some(b'\r')
            && bytes.get(newline + 2) == Some(&b'\n')
        {
            return Some((newline - 1, 4));
        }
        at = newline + 1;
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slot {
    Messages,
    Contents,
}

impl Slot {
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Messages => "messages",
            Self::Contents => "contents",
        }
    }
}

pub(crate) struct Encoding<'a> {
    pub(crate) reasoning: &'a Reasoning,
    pub(crate) cache: crate::endpoint::CacheTtl,
    pub(crate) stream: bool,
    pub(crate) keep_reasoning: bool,
    pub(crate) cache_marks: bool,
    pub(crate) assistant_after_tool: bool,
}

pub(crate) struct Wired {
    pub(crate) head: Value,
    pub(crate) messages: Option<Vec<u8>>,
    pub(crate) slot: Slot,
    pub(crate) raw_tools: bool,
}

impl Wired {
    pub(crate) fn head(head: Value) -> Self {
        Self {
            head,
            messages: None,
            slot: Slot::Messages,
            raw_tools: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn value(self) -> Result<Value> {
        let mut head = self.head;
        if let Some(messages) = self.messages {
            head[self.slot.key()] = serde_json::from_slice(&messages)?;
        }
        Ok(head)
    }
}

pub(crate) trait Accumulate {
    fn new(model: &str) -> Self;
    fn on(&mut self, frame: &str) -> Result<Vec<Delta>>;
    fn finish(self) -> AssistantMessage;
    fn reply(&self) -> &reply::ReplyBuilder;
    fn finished(&self) -> bool {
        self.reply().finished
    }
    fn thinking(&self) -> bool {
        false
    }
}

pub(crate) trait Chat {
    type Stream: Accumulate + Send;

    fn beta(_call: &CompletionRequest<'_>) -> Option<(&'static str, &'static str)> {
        None
    }

    fn body(call: &CompletionRequest<'_>, encoding: &Encoding<'_>) -> Result<Wired>;

    #[cfg(test)]
    fn request(call: &CompletionRequest<'_>, stream: bool) -> Result<Value> {
        Self::body(
            call,
            &Encoding {
                reasoning: &call.reasoning,
                cache: crate::limits::limits().cache_ttl,
                stream,
                keep_reasoning: false,
                cache_marks: false,
                assistant_after_tool: false,
            },
        )?
        .value()
    }
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "wire/framing_tests.rs"]
mod framing_tests;
