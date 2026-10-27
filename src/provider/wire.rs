use serde_json::Value;
use serde_json::value::RawValue;

use crate::error::{Error, ProviderFailureKind, Result};
use crate::provider::{AssistantMessage, CompletionRequest, Partial, Reasoning};

pub(crate) mod anthropic;
pub(crate) mod completions;
pub(crate) mod decisions;
pub(crate) mod gemini;
pub(crate) mod responses;
pub(crate) mod traits;

pub(crate) struct Spec {
    pub(crate) name: &'static str,
    pub(crate) headers: &'static [(&'static str, &'static str)],
    pub(crate) sentinel: Option<&'static str>,
    pub(crate) probe: Option<&'static str>,
    pub(crate) images: ImageBudget,
}

pub(crate) struct ImageBudget {
    pub(crate) types: &'static [&'static str],
    pub(crate) tool_types: &'static [&'static str],
    pub(crate) count: usize,
    pub(crate) each: usize,
    pub(crate) bytes: usize,
    pub(crate) tool_images: bool,
}

const PNG_JPEG: &[&str] = &["image/png", "image/jpeg"];
const WEB_IMAGES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];
const GEMINI_IMAGES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/heic",
    "image/heif",
];
const GEMINI_TOOL_IMAGES: &[&str] = &["image/png", "image/jpeg", "image/webp"];

const fn images(
    types: &'static [&'static str],
    count: usize,
    each: usize,
    bytes: usize,
) -> ImageBudget {
    ImageBudget {
        types,
        tool_types: types,
        count,
        each,
        bytes,
        tool_images: true,
    }
}

const SPEC: Spec = Spec {
    name: "completions",
    headers: &[],
    sentinel: None,
    probe: None,
    images: images(PNG_JPEG, 100, 10_000_000, 20_000_000),
};

pub(crate) fn spec(wire: crate::endpoint::Wire) -> Spec {
    let anthropic = images(WEB_IMAGES, 100, 10_000_000, 24_000_000);
    match wire {
        crate::endpoint::Wire::Anthropic => Spec {
            name: "anthropic",
            headers: &[("anthropic-version", "2023-06-01")],
            probe: Some("max_input_tokens"),
            images: anthropic,
            ..SPEC
        },
        crate::endpoint::Wire::AgentSdk => Spec {
            name: "agent-sdk",
            images: anthropic,
            ..SPEC
        },
        crate::endpoint::Wire::Gemini => Spec {
            name: "gemini",
            probe: Some("inputTokenLimit"),
            images: ImageBudget {
                tool_types: GEMINI_TOOL_IMAGES,
                tool_images: false,
                ..images(GEMINI_IMAGES, 3600, 15_000_000, 15_000_000)
            },
            ..SPEC
        },
        crate::endpoint::Wire::Responses => Spec {
            name: "responses",
            images: images(WEB_IMAGES, 1500, 256_000_000, 256_000_000),
            ..SPEC
        },
        crate::endpoint::Wire::Completions => Spec {
            sentinel: Some("[DONE]"),
            ..SPEC
        },
        crate::endpoint::Wire::Decisions => Spec {
            name: "decisions",
            sentinel: Some("[DONE]"),
            images: images(PNG_JPEG, 0, 0, 0),
            ..SPEC
        },
    }
}

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

const STATUS_MARKERS: &[(&str, u16)] = &[
    ("overload", 503),
    ("unavailable", 503),
    ("server", 503),
    ("api_error", 503),
    ("internal", 503),
    ("rate", 429),
    ("exhausted", 429),
];

pub(crate) fn failed(status: Option<u16>, kind: &str, message: &str) -> Error {
    let lower = kind.to_ascii_lowercase();
    let code = status.unwrap_or_else(|| {
        STATUS_MARKERS
            .iter()
            // complexity: bounded by the seven status markers
            .find(|(marker, _)| lower.contains(marker))
            .map_or(400, |(_, code)| *code)
    });
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

pub(crate) enum Delta {
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

pub(crate) struct Wired {
    pub(crate) head: Value,
    pub(crate) messages: Option<Vec<u8>>,
    pub(crate) slot: Slot,
}

impl Wired {
    pub(crate) fn head(head: Value) -> Self {
        Self {
            head,
            messages: None,
            slot: Slot::Messages,
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
    fn spill(&mut self, partial: &mut Partial);
    fn finished(&self) -> bool {
        false
    }
    fn thinking(&self) -> bool {
        false
    }
}

pub(crate) trait Chat {
    type Stream: Accumulate;

    fn path(model: &str, stream: bool) -> String;

    fn beta(_call: &CompletionRequest<'_>) -> Option<(&'static str, &'static str)> {
        None
    }

    fn body(
        call: &CompletionRequest<'_>,
        reasoning: &Reasoning,
        cache: crate::endpoint::CacheTtl,
        stream: bool,
    ) -> Result<Wired>;

    #[cfg(test)]
    fn request(call: &CompletionRequest<'_>, stream: bool) -> Result<Value> {
        Self::body(
            call,
            &call.reasoning,
            crate::limits::limits().cache_ttl,
            stream,
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
