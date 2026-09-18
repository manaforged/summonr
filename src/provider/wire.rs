#![allow(
    dead_code,
    reason = "the native wires are proved against the documented shapes and a \
              reference implementation, and nothing calls them until a key exists \
              to capture fixtures with and the dispatch moves off genai"
)]

use serde_json::Value;
use summonr_auth::Scheme;

use crate::provider::{AssistantMessage, CompletionRequest};

pub(crate) mod anthropic;
pub(crate) mod completions;
pub(crate) mod gemini;

pub(crate) fn arguments(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| serde_json::json!({}))
}

pub(crate) fn base64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub(crate) enum Delta {
    Text(String),
    Thinking(String),
    Call(String),
}

pub(crate) const MOST_PENDING: usize = 8 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct Frames {
    buffer: Vec<u8>,
    ended: bool,
}

impl Frames {
    pub(crate) fn push(
        &mut self,
        bytes: &[u8],
        sentinel: Option<&str>,
    ) -> crate::error::Result<Vec<Value>> {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > MOST_PENDING {
            return Err(crate::error::Error::provider(
                crate::error::ProviderFailureKind::Malformed,
                format!("{MOST_PENDING} bytes arrived with no frame boundary"),
            ));
        }
        let mut frames = Vec::new();
        while let Some((end, width)) = boundary(&self.buffer) {
            let event: Vec<u8> = self.buffer.drain(..end).collect();
            self.buffer.drain(..width);
            let text = std::str::from_utf8(&event).map_err(|error| {
                crate::error::Error::provider(
                    crate::error::ProviderFailureKind::Malformed,
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
                    self.ended = true;
                    return Ok(frames);
                }
                if let Ok(frame) = serde_json::from_str(data) {
                    frames.push(frame);
                }
            }
        }
        Ok(frames)
    }

    pub(crate) fn ended(&self) -> bool {
        self.ended
    }
}

pub(super) fn boundary(bytes: &[u8]) -> Option<(usize, usize)> {
    let lf = bytes.windows(2).position(|window| window == b"\n\n");
    let crlf = bytes.windows(4).position(|window| window == b"\r\n\r\n");
    match (lf, crlf) {
        (Some(left), Some(right)) if left <= right => Some((left, 2)),
        (Some(_), Some(right)) | (None, Some(right)) => Some((right, 4)),
        (Some(left), None) => Some((left, 2)),
        (None, None) => None,
    }
}

pub(crate) trait Accumulate {
    fn new(model: &str) -> Self;
    fn on(&mut self, frame: &Value) -> crate::error::Result<Vec<Delta>>;
    fn finish(self) -> AssistantMessage;
}

pub(crate) trait Chat {
    type Stream: Accumulate;

    fn path(model: &str, stream: bool) -> String;

    fn scheme() -> Scheme;

    fn headers() -> &'static [(&'static str, &'static str)] {
        &[]
    }

    fn sentinel() -> Option<&'static str> {
        None
    }

    fn request(call: &CompletionRequest, stream: bool) -> Value;

    fn parse(model: &str, body: &Value) -> AssistantMessage;
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "wire/framing_tests.rs"]
mod framing_tests;
