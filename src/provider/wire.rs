use serde_json::Value;

use crate::error::{Error, ProviderFailureKind, Result};
use crate::provider::{AssistantMessage, CompletionRequest};

pub(crate) mod anthropic;
pub(crate) mod completions;
pub(crate) mod decisions;
pub(crate) mod gemini;
pub(crate) mod responses;
pub(crate) mod traits;

pub(crate) fn headers(wire: crate::endpoint::Wire) -> &'static [(&'static str, &'static str)] {
    match wire {
        crate::endpoint::Wire::Anthropic => anthropic::Wire::headers(),
        crate::endpoint::Wire::Gemini => gemini::Wire::headers(),
        crate::endpoint::Wire::Responses => responses::Wire::headers(),
        crate::endpoint::Wire::Completions => completions::Wire::headers(),
        crate::endpoint::Wire::Decisions => &[],
    }
}

pub(crate) fn arguments(name: &str, raw: &str) -> Result<Value> {
    serde_json::from_str(raw).map_err(|error| {
        Error::provider(
            ProviderFailureKind::MalformedToolCall,
            format!("{name}: invalid tool arguments: {error}"),
        )
    })
}

pub(crate) fn failed(status: Option<u16>, kind: &str, message: &str) -> Error {
    let lower = kind.to_ascii_lowercase();
    let code = status.unwrap_or(
        if ["overload", "unavailable", "server", "api_error", "internal"]
            .iter()
            .any(|word| lower.contains(word))
        {
            503
        } else if lower.contains("rate") || lower.contains("exhausted") {
            429
        } else {
            400
        },
    );
    let mut error = super::client::response_failure(code, message, None);
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

pub(crate) enum Delta {
    Text(String),
    Thinking(String),
    Call(String),
}

pub(crate) const MOST_PENDING: usize = 64 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct Frames {
    buffer: Vec<u8>,
    scanned: usize,
    ended: bool,
}

impl Frames {
    pub(crate) fn push(&mut self, bytes: &[u8], sentinel: Option<&str>) -> Result<Vec<String>> {
        self.buffer.extend_from_slice(bytes);
        if self.buffer.len() > MOST_PENDING {
            return Err(Error::provider(
                ProviderFailureKind::Malformed,
                format!("{MOST_PENDING} bytes arrived with no frame boundary"),
            ));
        }
        let mut frames = Vec::new();
        while let Some((end, width)) = boundary(&self.buffer, self.scanned) {
            let event: Vec<u8> = self.buffer.drain(..end).collect();
            self.buffer.drain(..width);
            self.scanned = 0;
            let text = std::str::from_utf8(&event).map_err(|error| {
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
                    self.ended = true;
                    return Ok(frames);
                }
                frames.push(data.to_string());
            }
        }
        self.scanned = self.buffer.len().saturating_sub(3);
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

pub(super) fn boundary(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
    let bytes = &bytes[from.min(bytes.len())..];
    let lf = bytes.windows(2).position(|window| window == b"\n\n");
    let crlf = bytes.windows(4).position(|window| window == b"\r\n\r\n");
    match (lf, crlf) {
        (Some(left), Some(right)) if left <= right => Some((from + left, 2)),
        (Some(_), Some(right)) | (None, Some(right)) => Some((from + right, 4)),
        (Some(left), None) => Some((from + left, 2)),
        (None, None) => None,
    }
}

pub(crate) trait Accumulate {
    fn new(model: &str) -> Self;
    fn on(&mut self, frame: &str) -> Result<Vec<Delta>>;
    fn finish(self) -> AssistantMessage;
    fn finished(&self) -> bool {
        false
    }
}

pub(crate) trait Chat {
    type Stream: Accumulate;

    fn path(model: &str, stream: bool) -> String;

    fn headers() -> &'static [(&'static str, &'static str)] {
        &[]
    }

    fn sentinel() -> Option<&'static str> {
        None
    }

    fn request(call: &CompletionRequest<'_>, stream: bool) -> Result<Value>;

    #[cfg(test)]
    fn parse(model: &str, body: &Value) -> AssistantMessage;
}

#[cfg(test)]
#[path = "wire_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "wire/framing_tests.rs"]
mod framing_tests;
