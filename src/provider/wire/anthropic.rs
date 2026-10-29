#[cfg(test)]
use serde_json::Value;

use super::{Chat, Encoding, Wired};
use crate::provider::CompletionRequest;

mod body;

mod event;
mod part;
mod stream;

pub(crate) use event::Counted;
pub(crate) use event::{Change, Event, EventKind, Opened, Shape};
pub(crate) use stream::Stream;

const BINDING: &str = "thinking-binding-controls-2026-08-01";

pub(crate) struct Wire;

impl Chat for Wire {
    type Stream = Stream;

    fn beta(call: &CompletionRequest<'_>) -> Option<(&'static str, &'static str)> {
        call.traits.bound.then_some(("anthropic-beta", BINDING))
    }

    fn body(
        call: &CompletionRequest<'_>,
        encoding: &Encoding<'_>,
    ) -> summonr_core::error::Result<Wired> {
        body::body(call, encoding.reasoning, encoding.cache, encoding.stream)
    }
}

#[cfg(test)]
pub(crate) fn request(
    call: &CompletionRequest<'_>,
    stream: bool,
) -> summonr_core::error::Result<Value> {
    body::body(
        call,
        &call.reasoning,
        crate::limits::limits().cache_ttl,
        stream,
    )?
    .value()
}

#[cfg(test)]
#[path = "anthropic_tests.rs"]
pub(super) mod tests;
