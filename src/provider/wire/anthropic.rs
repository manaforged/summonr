#[cfg(test)]
use serde_json::Value;

use super::{Chat, Wired};
use crate::provider::{CompletionRequest, Reasoning, StopReason};

#[cfg(test)]
use crate::provider::Usage;

#[cfg(test)]
use serde_json::json;

#[cfg(test)]
use crate::provider::{AssistantMessage, Message, Role, ToolCall, ToolFunction, ToolKind};

mod body;

mod event;
mod part;
mod stream;

pub(crate) use event::Counted;
pub(crate) use stream::Stream;

pub(crate) struct Wire;

impl Chat for Wire {
    type Stream = Stream;

    fn path(_model: &str, _stream: bool) -> String {
        "/messages".to_string()
    }

    fn headers() -> &'static [(&'static str, &'static str)] {
        &[("anthropic-version", "2023-06-01")]
    }

    fn body(
        call: &CompletionRequest<'_>,
        reasoning: &Reasoning,
        cache: crate::endpoint::CacheTtl,
        stream: bool,
    ) -> crate::error::Result<Wired> {
        body::body(call, reasoning, cache, stream)
    }

    #[cfg(test)]
    fn parse(model: &str, body: &Value) -> AssistantMessage {
        parse(model, body)
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

#[cfg(test)]
pub(crate) fn parse(model: &str, body: &Value) -> AssistantMessage {
    let mut text = String::new();
    let mut thinking = String::new();
    let mut calls = Vec::new();
    for block in body
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => text.push_str(block.get("text").and_then(Value::as_str).unwrap_or("")),
            Some("thinking") => {
                thinking.push_str(block.get("thinking").and_then(Value::as_str).unwrap_or(""));
            }
            Some("tool_use") => calls.push(tool_call(block)),
            _ => {}
        }
    }
    if !thinking.is_empty()
        && let Some(call) = calls.first_mut()
        && let Some(blocks) = body.get("content").and_then(Value::as_array)
    {
        call.continuation = Some(crate::provider::Continuation::Anthropic {
            model: model.to_string(),
            blocks: blocks.clone(),
        });
    }
    AssistantMessage {
        content: (!text.is_empty()).then_some(text),
        tool_calls: calls,
        reasoning_content: (!thinking.is_empty()).then_some(thinking),
        usage: usage(body.get("usage")),
        stop_reason: stop_reason(body.get("stop_reason").and_then(Value::as_str)),
        continuation: None,
    }
}

#[cfg(test)]
fn tool_call(block: &Value) -> ToolCall {
    ToolCall {
        id: block
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: block
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            arguments: block
                .get("input")
                .map(ToString::to_string)
                .unwrap_or_else(|| "{}".to_string()),
            checked: crate::provider::message::Checked::Valid,
        },
        continuation: None,
    }
}

#[cfg(test)]
pub(crate) fn usage(value: Option<&Value>) -> Usage {
    value
        .and_then(|counted| serde_json::from_value::<event::Counted>(counted.clone()).ok())
        .unwrap_or_default()
        .usage()
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
