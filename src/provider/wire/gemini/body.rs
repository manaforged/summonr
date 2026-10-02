use serde::Serialize;
use serde_json::value::RawValue;
use serde_json::{Map, Value, json};

use super::super::{Slot, Wired};
use super::{SYNTHETIC, config};
use crate::provider::{CompletionRequest, Continuation, Message, Reasoning, Role};

#[derive(Serialize)]
struct Content<'a> {
    parts: Vec<Part<'a>>,
    role: &'static str,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Part<'a> {
    Value(Value),
    Call(Called<'a>),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Called<'a> {
    function_call: FunctionCall<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thought_signature: Option<&'a str>,
}

#[derive(Serialize)]
struct FunctionCall<'a> {
    args: &'a RawValue,
    name: &'a str,
}

const SKIP: &str = "skip_thought_signature_validator";

pub(super) fn body(
    call: &CompletionRequest<'_>,
    reasoning: &Reasoning,
) -> crate::error::Result<Wired> {
    let names: std::collections::HashMap<&str, &str> = call
        .messages
        .iter()
        .flat_map(|turn| &turn.tool_calls)
        .rev()
        .map(|call| (call.id.as_str(), call.function.name.as_str()))
        .collect();
    let mut system = String::new();
    let mut contents: Vec<Content<'_>> = Vec::new();
    for message in call.messages {
        match message.role {
            Role::System if !contents.is_empty() => {
                push(
                    &mut contents,
                    "user",
                    vec![Part::Value(json!({"text": message.content}))],
                );
            }
            Role::System => {
                if !system.is_empty() {
                    system.push('\n');
                }
                system.push_str(&message.content);
            }
            Role::User => push(
                &mut contents,
                "user",
                user_parts(message).into_iter().map(Part::Value).collect(),
            ),
            Role::Assistant => push(
                &mut contents,
                "model",
                model_parts(message, &call.model, call.traits.skip_signature)?,
            ),
            Role::Tool => push(
                &mut contents,
                "user",
                vec![Part::Value(response_part(message, &names))],
            ),
        }
    }
    let mut body = Map::new();
    if !system.is_empty() {
        body.insert(
            "systemInstruction".to_string(),
            json!({"parts": [{"text": system}]}),
        );
    }
    if !call.tools.is_empty() {
        body.insert(
            "tools".to_string(),
            json!([{"functionDeclarations": declarations(call)}]),
        );
    }
    body.insert(
        "generationConfig".to_string(),
        config::generation(call, reasoning),
    );
    Ok(Wired {
        head: Value::Object(body),
        messages: Some(serde_json::to_vec(&contents)?),
        slot: Slot::Contents,
    })
}

fn declarations(call: &CompletionRequest<'_>) -> Vec<Value> {
    call.tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.function.name,
                "description": tool.function.description,
                "parametersJsonSchema": tool.function.parameters,
            })
        })
        .collect()
}

fn push<'a>(contents: &mut Vec<Content<'a>>, role: &'static str, parts: Vec<Part<'a>>) {
    if parts.is_empty() {
        return;
    }
    if let Some(last) = contents.last_mut()
        && last.role == role
    {
        last.parts.extend(parts);
        return;
    }
    contents.push(Content { parts, role });
}

fn user_parts(message: &Message) -> Vec<Value> {
    let mut parts: Vec<Value> = message
        .images
        .iter()
        .map(|image| {
            json!({"inlineData": {"mimeType": image.media_type, "data": image.data.clone()}})
        })
        .collect();
    if !message.content.is_empty() {
        parts.push(json!({"text": message.content}));
    }
    parts
}

fn model_parts<'a>(
    message: &'a Message,
    model: &str,
    skip_signature: bool,
) -> crate::error::Result<Vec<Part<'a>>> {
    let first = message.tool_calls.first();
    let signatures: &[String] = if let Some(Continuation::Gemini {
        model: made,
        signatures,
    }) = first
        .and_then(|call| call.continuation.as_ref())
        .or(message.continuation.as_ref())
        && made == model
    {
        signatures
    } else {
        &[]
    };
    let signature = |slot: usize| {
        signatures
            .get(slot)
            .map(String::as_str)
            .filter(|found| !found.is_empty())
    };
    let mut parts = Vec::new();
    if !message.content.is_empty() {
        let mut text = json!({"text": message.content});
        if let Some(signed) = signature(message.tool_calls.len()) {
            text["thoughtSignature"] = json!(signed);
        }
        parts.push(Part::Value(text));
    }
    for (slot, call) in message.tool_calls.iter().enumerate() {
        let args = super::super::arguments(&call.function.name, &call.function.arguments)?;
        let skip = (signatures.is_empty() && skip_signature && slot == 0).then_some(SKIP);
        parts.push(Part::Call(Called {
            function_call: FunctionCall {
                args,
                name: &call.function.name,
            },
            thought_signature: signature(slot).or(skip),
        }));
    }
    Ok(parts)
}

fn response_part(message: &Message, names: &std::collections::HashMap<&str, &str>) -> Value {
    let id = message.tool_call_id.as_deref().unwrap_or_default();
    let name = names.get(id).copied().unwrap_or(id);
    let mut response =
        json!({"name": name, "response": {"name": name, "content": message.content}});
    if !id.starts_with(SYNTHETIC) {
        response["id"] = json!(id);
    }
    json!({"functionResponse": response})
}
