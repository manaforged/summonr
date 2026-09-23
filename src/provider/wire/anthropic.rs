use serde_json::{Map, Value, json};

use summonr_auth::Scheme;

use super::Chat;
use crate::provider::{
    AssistantMessage, CompletionRequest, Effort, Message, Role, StopReason, ToolCall, ToolFunction,
    ToolKind, Usage,
};

mod stream;

pub(crate) use stream::Stream;

pub(crate) struct Wire;

impl Chat for Wire {
    type Stream = Stream;

    fn path(_model: &str, _stream: bool) -> String {
        "/v1/messages".to_string()
    }

    fn scheme() -> Scheme {
        Scheme::ApiKey
    }

    fn headers() -> &'static [(&'static str, &'static str)] {
        &[("anthropic-version", "2023-06-01")]
    }

    fn request(call: &CompletionRequest<'_>, stream: bool) -> Value {
        request(call, stream)
    }

    fn parse(model: &str, body: &Value) -> AssistantMessage {
        parse(model, body)
    }
}

pub(crate) fn request(call: &CompletionRequest<'_>, stream: bool) -> Value {
    let mut system = Vec::new();
    let mut turns: Vec<Value> = Vec::new();
    for message in call.messages {
        match message.role {
            Role::System => system.push(json!({"type": "text", "text": message.content})),
            Role::Tool => push_result(&mut turns, message),
            Role::User => turns.push(json!({"role": "user", "content": user_parts(message)})),
            Role::Assistant => {
                turns.push(json!({"role": "assistant", "content": assistant_parts(message)}));
            }
        }
    }
    let mut body = Map::new();
    body.insert("model".to_string(), json!(call.model));
    body.insert(
        "max_tokens".to_string(),
        json!(call.max_output_tokens.unwrap_or(16_000)),
    );
    body.insert("messages".to_string(), Value::Array(turns));
    if !system.is_empty() {
        body.insert("system".to_string(), Value::Array(system));
    }
    if !call.tools.is_empty() {
        body.insert("tools".to_string(), Value::Array(tools(call)));
    }
    if stream {
        body.insert("stream".to_string(), json!(true));
    }
    if let Some(effort) = call.effort {
        body.insert(
            "output_config".to_string(),
            json!({"effort": effort_name(effort)}),
        );
        body.insert("thinking".to_string(), json!({"type": "adaptive"}));
    }
    Value::Object(body)
}

fn tools(call: &CompletionRequest<'_>) -> Vec<Value> {
    call.tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.function.name,
                "description": tool.function.description,
                "input_schema": tool.function.parameters,
            })
        })
        .collect()
}

fn user_parts(message: &Message) -> Vec<Value> {
    let mut parts: Vec<Value> = message
        .images
        .iter()
        .map(|image| {
            json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": image.media_type,
                    "data": image.data.clone(),
                },
            })
        })
        .collect();
    if !message.content.is_empty() {
        parts.push(json!({"type": "text", "text": message.content}));
    }
    parts
}

fn assistant_parts(message: &Message) -> Vec<Value> {
    if let Some(kept) = retained(message) {
        return kept;
    }
    let mut parts = Vec::new();
    if !message.content.is_empty() {
        parts.push(json!({"type": "text", "text": message.content}));
    }
    for call in &message.tool_calls {
        parts.push(json!({
            "type": "tool_use",
            "id": call.id,
            "name": call.function.name,
            "input": super::arguments(&call.function.arguments),
        }));
    }
    parts
}

fn retained(message: &Message) -> Option<Vec<Value>> {
    let blocks =
        message
            .tool_calls
            .iter()
            .find_map(|call| match call.continuation.as_ref()? {
                crate::provider::Continuation::Anthropic { blocks, .. } => Some(blocks),
                _ => None,
            })?;
    (!blocks.is_empty()).then(|| blocks.clone())
}

fn push_result(turns: &mut Vec<Value>, message: &Message) {
    let block = json!({
        "type": "tool_result",
        "tool_use_id": message.tool_call_id.clone().unwrap_or_default(),
        "content": message.content,
    });
    if let Some(Value::Object(last)) = turns.last_mut()
        && last.get("role").and_then(Value::as_str) == Some("user")
        && let Some(Value::Array(parts)) = last.get_mut("content")
        && parts
            .first()
            .and_then(|part| part.get("type"))
            .and_then(Value::as_str)
            == Some("tool_result")
    {
        parts.push(block);
        return;
    }
    turns.push(json!({"role": "user", "content": [block]}));
}

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
    }
}

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
        },
        continuation: None,
    }
}

pub(crate) fn usage(value: Option<&Value>) -> Usage {
    let Some(value) = value else {
        return Usage::default();
    };
    let field = |name: &str| {
        value
            .get(name)
            .and_then(Value::as_u64)
            .unwrap_or_default()
            .min(u64::from(u32::MAX)) as u32
    };
    Usage {
        prompt_tokens: field("input_tokens") + field("cache_creation_input_tokens"),
        completion_tokens: field("output_tokens"),
        reasoning_tokens: 0,
        cached_tokens: field("cache_read_input_tokens"),
    }
}

pub(crate) fn stop_reason(raw: Option<&str>) -> StopReason {
    match raw {
        Some("tool_use") => StopReason::ToolCalls,
        Some("max_tokens") => StopReason::Length,
        _ => StopReason::Stop,
    }
}

fn effort_name(effort: Effort) -> &'static str {
    match effort {
        Effort::None | Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
    }
}

#[cfg(test)]
#[path = "anthropic_tests.rs"]
pub(super) mod tests;
