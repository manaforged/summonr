use serde_json::{Map, Value, json};

use stream::Stream;

use super::Chat;
use crate::provider::{CompletionRequest, Continuation, Effort, Message, Reasoning, Role, Step};

#[cfg(test)]
use super::Accumulate;
#[cfg(test)]
use crate::provider::AssistantMessage;

pub(crate) struct Wire;

impl Chat for Wire {
    type Stream = Stream;

    fn path(_model: &str, _stream: bool) -> String {
        "/responses".to_string()
    }

    fn body(
        call: &CompletionRequest<'_>,
        reasoning: &Reasoning,
        _cache: crate::endpoint::CacheTtl,
        stream: bool,
    ) -> crate::error::Result<super::Wired> {
        body(call, reasoning, stream).map(super::Wired::head)
    }

    #[cfg(test)]
    fn parse(model: &str, body: &Value) -> AssistantMessage {
        let mut reply = Stream::new(model);
        drop(reply.whole(body));
        reply.finish()
    }
}

#[cfg(test)]
pub(crate) fn request(call: &CompletionRequest<'_>, stream: bool) -> crate::error::Result<Value> {
    body(call, &call.reasoning, stream)
}

pub(crate) fn body(
    call: &CompletionRequest<'_>,
    reasoning: &Reasoning,
    stream: bool,
) -> crate::error::Result<Value> {
    let (instructions, input) = input(call)?;
    let mut body = Map::new();
    body.insert("model".to_string(), json!(call.model));
    body.insert("input".to_string(), Value::Array(input));
    if !instructions.is_empty() {
        body.insert("instructions".to_string(), json!(instructions));
    }
    if !call.tools.is_empty() {
        body.insert("tools".to_string(), Value::Array(tools(call)));
    }
    if let Some(max) = call.max_output_tokens {
        body.insert("max_output_tokens".to_string(), json!(max));
    }
    if stream {
        body.insert("stream".to_string(), json!(true));
    }
    effort(&mut body, call, reasoning);
    body.insert("store".to_string(), json!(false));
    Ok(Value::Object(body))
}

fn input(call: &CompletionRequest<'_>) -> crate::error::Result<(String, Vec<Value>)> {
    let mut instructions = String::new();
    let mut input: Vec<Value> = Vec::new();
    for message in call.messages {
        match message.role {
            Role::System if !input.is_empty() => input.push(json!({
                "type": "message",
                "role": "developer",
                "content": [{"type": "input_text", "text": message.content}],
            })),
            Role::System => {
                if !instructions.is_empty() {
                    instructions.push('\n');
                }
                instructions.push_str(&message.content);
            }
            Role::User => input.push(json!({
                "type": "message",
                "role": "user",
                "content": user_parts(message),
            })),
            Role::Assistant => input.extend(assistant_items(message, &call.model)?),
            Role::Tool => input.push(json!({
                "type": "function_call_output",
                "call_id": call_id(message.tool_call_id.as_deref().unwrap_or_default())?,
                "output": tool_output(message),
            })),
        }
    }
    Ok((instructions, input))
}

fn effort(body: &mut Map<String, Value>, call: &CompletionRequest<'_>, reasoning: &Reasoning) {
    match reasoning.plan(call.traits).step(call.effort) {
        Step::Level(Effort::None) => {
            body.insert("reasoning".to_string(), json!({"effort": "none"}));
        }
        Step::Level(level) => {
            body.insert(
                "reasoning".to_string(),
                json!({"effort": level.as_str(), "summary": "auto"}),
            );
            body.insert(
                "include".to_string(),
                json!(["reasoning.encrypted_content"]),
            );
        }
        Step::Default | Step::Off(_) => {}
    }
}

const NAMESPACE: &str = "summonr";

pub(crate) fn namespace(tools: Value) -> Value {
    json!({
        "type": "namespace",
        "name": NAMESPACE,
        "description": "Tools of the Summonr coding agent",
        "tools": tools,
    })
}

fn tools(call: &CompletionRequest<'_>) -> Vec<Value> {
    call.tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "name": tool.function.name,
                "description": tool.function.description,
                "parameters": tool.function.parameters,
                "strict": false,
            })
        })
        .collect()
}

fn tool_output(message: &Message) -> Value {
    if message.images.is_empty() {
        return json!(message.content);
    }
    let mut parts = vec![json!({"type": "input_text", "text": message.content})];
    parts.extend(message.images.iter().map(input_image));
    Value::Array(parts)
}

fn input_image(image: &crate::provider::Image) -> Value {
    json!({"type": "input_image", "image_url": image.url()})
}

fn user_parts(message: &Message) -> Vec<Value> {
    let mut parts: Vec<Value> = message.images.iter().map(input_image).collect();
    if !message.content.is_empty() {
        parts.push(json!({"type": "input_text", "text": message.content}));
    }
    parts
}

fn assistant_items(message: &Message, model: &str) -> crate::error::Result<Vec<Value>> {
    let mut items = Vec::new();
    let mut phase = None;
    let mut remarks: &[String] = &[];
    let calls = message.tool_calls.iter().map(|call| &call.continuation);
    for continuation in calls.chain([&message.continuation]) {
        if let Some(Continuation::Responses {
            model: made,
            signatures,
            phase: said,
            commentary,
        }) = continuation
            && made == model
        {
            phase = phase.or(said.as_ref());
            remarks = if remarks.is_empty() {
                commentary
            } else {
                remarks
            };
            for signature in signatures {
                items.push(json!({
                    "type": "reasoning",
                    "summary": [],
                    "encrypted_content": signature,
                }));
            }
        }
    }
    for remark in remarks.iter().filter(|remark| !remark.is_empty()) {
        items.push(json!({
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": remark}],
            "phase": "commentary",
        }));
    }
    if !message.content.is_empty() {
        let mut item = json!({
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": message.content}],
        });
        if let Some(phase) = phase {
            item["phase"] = json!(phase);
        }
        items.push(item);
    }
    for call in &message.tool_calls {
        super::checked(&call.function)?;
        items.push(json!({
            "type": "function_call",
            "call_id": call_id(&call.id)?,
            "name": call.function.name,
            "arguments": call.function.arguments,
        }));
    }
    Ok(items)
}

fn call_id(id: &str) -> crate::error::Result<&str> {
    if id.is_empty() {
        return Err(crate::error::Error::provider(
            crate::error::ProviderFailureKind::MalformedToolCall,
            "responses: a tool call has an empty call_id",
        ));
    }
    Ok(id)
}

mod frame;
mod stream;

#[cfg(test)]
#[path = "responses_tests.rs"]
mod tests;
