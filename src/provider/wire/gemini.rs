use serde_json::{Map, Value, json};

use summonr_auth::Scheme;

use super::{Accumulate, Chat, Delta};
use crate::provider::{
    AssistantMessage, CompletionRequest, Effort, Message, Role, StopReason, ToolCall, ToolFunction,
    ToolKind, Usage,
};

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

    fn scheme() -> Scheme {
        Scheme::GoogleKey
    }

    fn request(call: &CompletionRequest, _stream: bool) -> Value {
        request(call)
    }

    fn parse(model: &str, body: &Value) -> AssistantMessage {
        let mut reply = <Reply as Accumulate>::new(model);
        drop(reply.on(body));
        reply.finish()
    }
}

pub(crate) fn request(call: &CompletionRequest) -> Value {
    let mut system = String::new();
    let mut contents: Vec<Value> = Vec::new();
    for message in &call.messages {
        match message.role {
            Role::System => {
                if !system.is_empty() {
                    system.push('\n');
                }
                system.push_str(&message.content);
            }
            Role::User => push(&mut contents, "user", user_parts(message)),
            Role::Assistant => push(&mut contents, "model", model_parts(message)),
            Role::Tool => push(&mut contents, "user", vec![response_part(message)]),
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
    body.insert("generationConfig".to_string(), generation(call));
    Value::Object(body)
}

fn generation(call: &CompletionRequest) -> Value {
    let mut config = Map::new();
    if let Some(limit) = call.max_output_tokens {
        config.insert("maxOutputTokens".to_string(), json!(limit));
    }
    if let Some(effort) = call.effort {
        config.insert(
            "thinkingConfig".to_string(),
            json!({"thinkingLevel": level(effort), "includeThoughts": true}),
        );
    }
    Value::Object(config)
}

fn level(effort: Effort) -> &'static str {
    match effort {
        Effort::None | Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
    }
}

fn declarations(call: &CompletionRequest) -> Vec<Value> {
    call.tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.function.name,
                "description": tool.function.description,
                "parameters": tool.function.parameters,
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
            json!({"inlineData": {"mimeType": image.media_type, "data": super::base64(&image.bytes)}})
        })
        .collect();
    if !message.content.is_empty() {
        parts.push(json!({"text": message.content}));
    }
    parts
}

fn model_parts(message: &Message) -> Vec<Value> {
    let mut parts = Vec::new();
    if !message.content.is_empty() {
        parts.push(json!({"text": message.content}));
    }
    for call in &message.tool_calls {
        parts.push(json!({
            "functionCall": {"name": call.function.name, "args": super::arguments(&call.function.arguments)},
        }));
        if let Some(crate::provider::Continuation::Gemini { signatures, .. }) = &call.continuation {
            for signature in signatures {
                parts.push(json!({"thoughtSignature": signature}));
            }
        }
    }
    parts
}

fn response_part(message: &Message) -> Value {
    let id = message.tool_call_id.clone().unwrap_or_default();
    let name = called_name(&id);
    json!({
        "functionResponse": {
            "name": name,
            "response": {"name": name, "content": message.content},
        }
    })
}

pub(crate) fn called_name(id: &str) -> &str {
    let Some(rest) = id.strip_prefix(SYNTHETIC) else {
        return id;
    };
    match rest.rsplit_once('-') {
        Some((name, counter)) if !name.is_empty() && counter.parse::<usize>().is_ok() => name,
        _ => id,
    }
}

#[derive(Default)]
pub(crate) struct Reply {
    pub(crate) model: String,
    text: String,
    thinking: String,
    pub(crate) signatures: Vec<String>,
    calls: Vec<ToolCall>,
    usage: Usage,
    stop: StopReason,
}

impl Accumulate for Reply {
    fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            ..Self::default()
        }
    }

    fn on(&mut self, frame: &Value) -> crate::error::Result<Vec<Delta>> {
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
        Ok(deltas)
    }

    fn finish(mut self) -> AssistantMessage {
        if !self.calls.is_empty() {
            self.stop = StopReason::ToolCalls;
            let signatures = std::mem::take(&mut self.signatures);
            if !signatures.is_empty()
                && let Some(call) = self.calls.first_mut()
            {
                call.continuation = Some(crate::provider::Continuation::Gemini {
                    model: self.model.clone(),
                    signatures,
                });
            }
        }
        AssistantMessage {
            content: (!self.text.is_empty()).then_some(self.text),
            tool_calls: self.calls,
            reasoning_content: (!self.thinking.is_empty()).then_some(self.thinking),
            usage: self.usage,
            stop_reason: self.stop,
        }
    }
}

impl Reply {
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
            if let Some(signature) = part.get("thoughtSignature").and_then(Value::as_str) {
                self.signatures.push(signature.to_string());
            }
            if let Some(call) = part.get("functionCall") {
                self.calls.push(self.called(call));
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
    let cached = field("cachedContentTokenCount");
    Usage {
        prompt_tokens: field("promptTokenCount").saturating_sub(cached),
        completion_tokens: field("candidatesTokenCount"),
        reasoning_tokens: field("thoughtsTokenCount"),
        cached_tokens: cached,
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
