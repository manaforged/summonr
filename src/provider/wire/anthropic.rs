#[cfg(test)]
use serde_json::Value;

use super::{Chat, Wired};
use crate::provider::{CompletionRequest, Reasoning, StopReason};

mod body;

mod event;
mod part;
mod stream;

pub(crate) use event::Counted;
pub(crate) use stream::Stream;

const BINDING: &str = "thinking-binding-controls-2026-08-01";

pub(crate) struct Wire;

impl Chat for Wire {
    type Stream = Stream;

    fn path(_model: &str, _stream: bool) -> String {
        "/messages".to_string()
    }

    fn headers() -> &'static [(&'static str, &'static str)] {
        &[("anthropic-version", "2023-06-01")]
    }

    fn beta(call: &CompletionRequest<'_>) -> Option<(&'static str, &'static str)> {
        call.traits.bound.then_some(("anthropic-beta", BINDING))
    }

    fn body(
        call: &CompletionRequest<'_>,
        reasoning: &Reasoning,
        cache: crate::endpoint::CacheTtl,
        stream: bool,
    ) -> crate::error::Result<Wired> {
        body::body(call, reasoning, cache, stream)
    }
}

#[cfg(test)]
pub(crate) fn request(call: &CompletionRequest<'_>, stream: bool) -> crate::error::Result<Value> {
    body::body(
        call,
        &call.reasoning,
        crate::limits::limits().cache_ttl,
        stream,
    )?
    .value()
}

pub(crate) fn stop_reason(raw: Option<&str>) -> StopReason {
    match raw {
        Some("tool_use") => StopReason::ToolCalls,
        Some("max_tokens" | "model_context_window_exceeded") => StopReason::Length,
        _ => StopReason::Stop,
    }
}

#[cfg(test)]
#[path = "anthropic_tests.rs"]
pub(super) mod tests;
