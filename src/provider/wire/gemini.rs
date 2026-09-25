use serde_json::{Map, Value, json};

use super::{Accumulate, Chat, Delta};
use crate::provider::{
    AssistantMessage, CompletionRequest, Continuation, Message, Role, StopReason, ToolCall,
    ToolFunction, ToolKind, Usage,
};

mod config;

const SYNTHETIC: &str = "call-";

pub(crate) struct Wire;

impl Chat for Wire {
    type Stream = Reply;

    fn path(model: &str, stream: bool) -> String {
        if stream {
            format!("/models/{model}:streamGenerateContent?alt=sse")
        } else {
            format!("/models/{model}:generateContent")
        }
    }

    fn request(call: &CompletionRequest<'_>, _stream: bool) -> crate::error::Result<Value> {
        request(call)
    }

    #[cfg(test)]
    fn parse(model: &str, body: &Value) -> AssistantMessage {
        let mut reply = <Reply as Accumulate>::new(model);
        drop(reply.value(body));
        reply.finish()
    }
}

pub(crate) fn request(call: &CompletionRequest<'_>) -> crate::error::Result<Value> {
    let mut system = String::new();
    let mut contents: Vec<Value> = Vec::new();
    for message in call.messages {
        match message.role {
            Role::System if !contents.is_empty() => {
                push(
                    &mut contents,
                    "user",
                    vec![json!({"text": message.content})],
                );
            }
            Role::System => {
                if !system.is_empty() {
                    system.push('\n');
                }
                system.push_str(&message.content);
            }
            Role::User => push(&mut contents, "user", user_parts(message)),
            Role::Assistant => push(&mut contents, "model", model_parts(message, &call.model)?),
            Role::Tool => push(
                &mut contents,
                "user",
                vec![response_part(message, call.messages)],
            ),
        }
    }
    let mut body = Map::new();
    body.insert("contents".to_string(), Value::Array(contents));
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
    body.insert("generationConfig".to_string(), config::generation(call));
    Ok(Value::Object(body))
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

fn push(contents: &mut Vec<Value>, role: &str, parts: Vec<Value>) {
    if parts.is_empty() {
        return;
    }
    if let Some(Value::Object(last)) = contents.last_mut()
        && last.get("role").and_then(Value::as_str) == Some(role)
        && let Some(Value::Array(kept)) = last.get_mut("parts")
    {
        kept.extend(parts);
        return;
    }
    contents.push(json!({"role": role, "parts": parts}));
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

fn model_parts(message: &Message, model: &str) -> crate::error::Result<Vec<Value>> {
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
    let foreign = signatures.is_empty() && super::traits::traits(model).skip_signature;
    let signed = |mut part: Value, slot: usize| {
        if let Some(signature) = signatures.get(slot).filter(|found| !found.is_empty()) {
            part["thoughtSignature"] = json!(signature);
        } else if foreign && slot == 0 && part.get("functionCall").is_some() {
            part["thoughtSignature"] = json!("skip_thought_signature_validator");
        }
        part
    };
    let mut parts = Vec::new();
    if !message.content.is_empty() {
        let text = json!({"text": message.content});
        parts.push(signed(text, message.tool_calls.len()));
    }
    for (slot, call) in message.tool_calls.iter().enumerate() {
        let args = super::arguments(&call.function.name, &call.function.arguments)?;
        let part = json!({"functionCall": {"name": call.function.name, "args": args}});
        parts.push(signed(part, slot));
    }
    Ok(parts)
}

fn response_part(message: &Message, history: &[Message]) -> Value {
    let id = message.tool_call_id.as_deref().unwrap_or_default();
    let name = history
        .iter()
        .flat_map(|turn| &turn.tool_calls)
        .find(|call| call.id == id)
        .map_or(id, |call| call.function.name.as_str());
    let mut response =
        json!({"name": name, "response": {"name": name, "content": message.content}});
    if !id.starts_with(SYNTHETIC) {
        response["id"] = json!(id);
    }
    json!({"functionResponse": response})
}

#[derive(Default)]
pub(crate) struct Reply {
    pub(crate) model: String,
    text: String,
    thinking: String,
    pub(crate) signatures: Vec<String>,
    pub(crate) said: String,
    calls: Vec<ToolCall>,
    usage: Usage,
    stop: StopReason,
    done: bool,
}

impl Accumulate for Reply {
    fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            ..Self::default()
        }
    }

    fn on(&mut self, frame: &str) -> crate::error::Result<Vec<Delta>> {
        let frame = crate::provider::wire::parse::<Value>(frame)?;
        if let Some(error) = frame.get("error") {
            return Err(super::failed(
                error
                    .get("code")
                    .and_then(Value::as_u64)
                    .and_then(|code| u16::try_from(code).ok()),
                error.get("status").and_then(Value::as_str).unwrap_or(""),
                error.get("message").and_then(Value::as_str).unwrap_or(""),
            ));
        }
        if let Some(reason) = frame
            .pointer("/promptFeedback/blockReason")
            .and_then(Value::as_str)
        {
            return Err(super::blocked(reason));
        }
        if let Some(reason) = frame
            .pointer("/candidates/0/finishReason")
            .and_then(Value::as_str)
        {
            self.done = true;
            if matches!(
                reason,
                "SAFETY"
                    | "RECITATION"
                    | "BLOCKLIST"
                    | "PROHIBITED_CONTENT"
                    | "SPII"
                    | "IMAGE_SAFETY"
            ) {
                return Err(super::blocked(reason));
            }
            if reason == "MALFORMED_FUNCTION_CALL" {
                return Err(crate::error::Error::provider(
                    crate::error::ProviderFailureKind::MalformedToolCall,
                    format!("native response ended with {reason}"),
                ));
            }
        }
        Ok(self.value(&frame))
    }

    fn finished(&self) -> bool {
        self.done
    }

    fn finish(mut self) -> AssistantMessage {
        if !self.calls.is_empty() {
            self.stop = StopReason::ToolCalls;
            let mut signatures = std::mem::take(&mut self.signatures);
            if !self.said.is_empty() {
                signatures.push(std::mem::take(&mut self.said));
            }
            if signatures.iter().any(|signature| !signature.is_empty())
                && let Some(call) = self.calls.first_mut()
            {
                call.continuation = Some(Continuation::Gemini {
                    model: self.model.clone(),
                    signatures,
                });
            }
        }
        let continuation =
            (self.calls.is_empty() && !self.said.is_empty()).then(|| Continuation::Gemini {
                model: self.model.clone(),
                signatures: vec![std::mem::take(&mut self.said)],
            });
        AssistantMessage {
            continuation,
            content: (!self.text.is_empty()).then_some(self.text),
            tool_calls: self.calls,
            reasoning_content: (!self.thinking.is_empty()).then_some(self.thinking),
            usage: self.usage,
            stop_reason: self.stop,
        }
    }
}

impl Reply {
    fn value(&mut self, frame: &Value) -> Vec<Delta> {
        let thought = self.thinking.len();
        let named = self.calls.len();
        let text = self.take(frame);
        let mut deltas = Vec::new();
        if self.thinking.len() > thought {
            deltas.push(Delta::Thinking(self.thinking[thought..].to_string()));
        }
        if let Some(chunk) = text {
            deltas.push(Delta::Text(chunk));
        }
        for call in self.calls.iter().skip(named) {
            deltas.push(Delta::Call(call.function.name.clone()));
        }
        deltas
    }

    pub(crate) fn take(&mut self, body: &Value) -> Option<String> {
        if let Some(counted) = body.get("usageMetadata") {
            self.usage = usage(counted);
        }
        if let Some(reason) = body
            .pointer("/candidates/0/finishReason")
            .and_then(Value::as_str)
        {
            self.stop = stop_reason(reason);
        }
        let mut chunk = String::new();
        for part in body
            .pointer("/candidates/0/content/parts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let signature = part.get("thoughtSignature").and_then(Value::as_str);
            if let Some(call) = part.get("functionCall") {
                self.calls.push(self.called(call));
                self.signatures
                    .push(signature.unwrap_or_default().to_string());
            } else if let Some(signature) = signature {
                self.said = signature.to_string();
            }
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                if part.get("thought").and_then(Value::as_bool) == Some(true) {
                    self.thinking.push_str(text);
                } else {
                    self.text.push_str(text);
                    chunk.push_str(text);
                }
            }
        }
        (!chunk.is_empty()).then_some(chunk)
    }

    fn called(&self, call: &Value) -> ToolCall {
        let name = call
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let id = call.get("id").and_then(Value::as_str).map_or_else(
            || format!("{SYNTHETIC}{name}-{}", self.calls.len()),
            str::to_string,
        );
        ToolCall {
            id,
            kind: ToolKind::Function,
            function: ToolFunction {
                name,
                arguments: call
                    .get("args")
                    .filter(|args| !args.is_null())
                    .map_or_else(|| "{}".to_string(), ToString::to_string),
            },
            continuation: None,
        }
    }
}

pub(crate) fn usage(counted: &Value) -> Usage {
    let field = |name: &str| {
        counted
            .get(name)
            .and_then(Value::as_u64)
            .unwrap_or_default()
            .min(u64::from(u32::MAX)) as u32
    };
    Usage {
        prompt_tokens: field("promptTokenCount"),
        completion_tokens: field("candidatesTokenCount") + field("thoughtsTokenCount"),
        reasoning_tokens: field("thoughtsTokenCount"),
        cached_tokens: field("cachedContentTokenCount"),
    }
}

pub(crate) fn stop_reason(raw: &str) -> StopReason {
    match raw {
        "MAX_TOKENS" => StopReason::Length,
        "STOP" => StopReason::Stop,
        _ => StopReason::Other,
    }
}

#[cfg(test)]
#[path = "gemini_tests.rs"]
mod tests;
