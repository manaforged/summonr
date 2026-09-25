use serde_json::{Map, Value, json};

use super::Chat;
use super::traits::{Thinking, traits};
use crate::provider::{CompletionRequest, Effort, Message, Role, StopReason, Usage};

#[cfg(test)]
use crate::provider::{AssistantMessage, ToolCall, ToolFunction, ToolKind};

mod stream;

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

    fn request(call: &CompletionRequest<'_>, stream: bool) -> crate::error::Result<Value> {
        request(call, stream)
    }

    #[cfg(test)]
    fn parse(model: &str, body: &Value) -> AssistantMessage {
        parse(model, body)
    }
}

pub(crate) fn request(call: &CompletionRequest<'_>, stream: bool) -> crate::error::Result<Value> {
    let mut system = Vec::new();
    let mut turns: Vec<Value> = Vec::new();
    for message in call.messages {
        match message.role {
            Role::System if turns.is_empty() => {
                system.push(json!({"type": "text", "text": message.content}));
            }
            Role::System => push_user(
                &mut turns,
                vec![json!({"type": "text", "text": message.content})],
            ),
            Role::Tool => push_result(&mut turns, message),
            Role::User => push_user(&mut turns, user_parts(message)),
            Role::Assistant => {
                let parts = assistant_parts(message, &call.model)?;
                if !parts.is_empty() {
                    turns.push(json!({"role": "assistant", "content": parts}));
                }
            }
        }
    }
    if let Some(last) = system.last_mut() {
        breakpoint(last);
    }
    let mut tools = tools(call);
    if let Some(last) = tools.last_mut() {
        breakpoint(last);
    }
    if let Some(Value::Object(turn)) = turns.last_mut()
        && let Some(Value::Array(parts)) = turn.get_mut("content")
        && let Some(last) = parts.iter_mut().rev().find(|part| {
            !matches!(
                part.get("type").and_then(Value::as_str),
                Some("thinking" | "redacted_thinking")
            )
        })
    {
        breakpoint(last);
    }
    let unthought = turns.len() > 1
        && turns[turns.len() - 1]["content"][0]["type"] == "tool_result"
        && !matches!(
            turns[turns.len() - 2]["content"][0]["type"].as_str(),
            Some("thinking" | "redacted_thinking")
        );
    let mut body = Map::new();
    body.insert("model".to_string(), json!(call.model));
    let most = call.max_output_tokens.unwrap_or(16_000);
    body.insert("max_tokens".to_string(), json!(most));
    body.insert("messages".to_string(), Value::Array(turns));
    if !system.is_empty() {
        body.insert("system".to_string(), Value::Array(system));
    }
    if !tools.is_empty() {
        body.insert("tools".to_string(), Value::Array(tools));
    }
    if stream {
        body.insert("stream".to_string(), json!(true));
    }
    if let Some(effort) = call
        .effort
        .filter(|effort| *effort != Effort::None && !unthought)
    {
        let thinking = traits(&call.model).thinking;
        if thinking == Thinking::Adaptive {
            body.insert(
                "output_config".to_string(),
                json!({"effort": effort_name(effort)}),
            );
            body.insert("thinking".to_string(), json!({"type": "adaptive"}));
        } else if thinking == Thinking::Budgeted {
            let budget = most / 4 * effort as u32;
            if (1024..most).contains(&budget) {
                body.insert(
                    "thinking".to_string(),
                    json!({"type": "enabled", "budget_tokens": budget}),
                );
            }
        }
    }
    Ok(Value::Object(body))
}

fn breakpoint(part: &mut Value) {
    part["cache_control"] = json!({"type": "ephemeral"});
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

fn assistant_parts(message: &Message, model: &str) -> crate::error::Result<Vec<Value>> {
    if let Some(kept) = retained(message, model)? {
        return Ok(kept);
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
            "input": super::arguments(&call.function.name, &call.function.arguments)?,
        }));
    }
    Ok(parts)
}

fn retained(message: &Message, model: &str) -> crate::error::Result<Option<Vec<Value>>> {
    let Some(mut blocks) = message.tool_calls.iter().find_map(|call| {
        if let Some(crate::provider::Continuation::Anthropic {
            model: made,
            blocks,
        }) = call.continuation.as_ref()
            && made == model
        {
            Some(blocks.clone())
        } else {
            None
        }
    }) else {
        return Ok(None);
    };
    let mut calls = message.tool_calls.iter();
    for block in blocks
        .iter_mut()
        .filter(|block| block["type"] == "tool_use")
    {
        let Some(call) = calls.next().filter(|call| block["id"] == call.id.as_str()) else {
            return Ok(None);
        };
        block["name"] = json!(call.function.name);
        block["input"] = super::arguments(&call.function.name, &call.function.arguments)?;
    }
    Ok(calls.next().is_none().then_some(blocks))
}

fn push_result(turns: &mut Vec<Value>, message: &Message) {
    let mut block = json!({
        "type": "tool_result",
        "tool_use_id": message.tool_call_id.clone().unwrap_or_default(),
        "content": user_parts(message),
    });
    if message.failed {
        block["is_error"] = json!(true);
    }
    let paired = turns.len() > 1 && turns[turns.len() - 2]["role"] == "assistant";
    let merged = turns
        .last()
        .is_some_and(|turn| turn["content"][0]["type"] == "tool_result");
    if (paired || merged)
        && let Some(Value::Object(last)) = turns.last_mut()
        && last.get("role").and_then(Value::as_str) == Some("user")
        && let Some(Value::Array(parts)) = last.get_mut("content")
    {
        let at = parts
            .iter()
            .take_while(|part| part.get("type").and_then(Value::as_str) == Some("tool_result"))
            .count();
        parts.insert(at, block);
        return;
    }
    turns.push(json!({"role": "user", "content": [block]}));
}

fn push_user(turns: &mut Vec<Value>, mut added: Vec<Value>) {
    if added.is_empty() {
        return;
    }
    if let Some(Value::Object(last)) = turns.last_mut()
        && last.get("role").and_then(Value::as_str) == Some("user")
        && let Some(Value::Array(parts)) = last.get_mut("content")
    {
        parts.append(&mut added);
        return;
    }
    turns.push(json!({"role": "user", "content": added}));
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
        prompt_tokens: field("input_tokens")
            + field("cache_creation_input_tokens")
            + field("cache_read_input_tokens"),
        completion_tokens: field("output_tokens"),
        reasoning_tokens: value
            .pointer("/output_tokens_details/thinking_tokens")
            .and_then(Value::as_u64)
            .unwrap_or_default()
            .min(u64::from(u32::MAX)) as u32,
        cached_tokens: field("cache_read_input_tokens"),
    }
}

pub(crate) fn stop_reason(raw: Option<&str>) -> StopReason {
    match raw {
        Some("tool_use") => StopReason::ToolCalls,
        Some("max_tokens" | "model_context_window_exceeded") => StopReason::Length,
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
