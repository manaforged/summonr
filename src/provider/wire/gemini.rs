#[cfg(test)]
use serde_json::Value;

use super::{Accumulate, Chat, Delta};
use crate::provider::message::Checked;
use crate::provider::{
    AssistantMessage, CompletionRequest, Continuation, Partial, Reasoning, StopReason, ToolCall,
    ToolFunction, ToolKind, Usage,
};
#[cfg(test)]
use frame::Counted;
use frame::{Call, Chunk};

#[cfg(test)]
use crate::provider::{Message, Role};
#[cfg(test)]
use serde_json::json;

mod body;
mod config;
mod frame;

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

    fn body(
        call: &CompletionRequest<'_>,
        reasoning: &Reasoning,
        _cache: crate::endpoint::CacheTtl,
        _stream: bool,
    ) -> crate::error::Result<super::Wired> {
        body::body(call, reasoning)
    }

    #[cfg(test)]
    fn parse(model: &str, body: &Value) -> AssistantMessage {
        let mut reply = <Reply as Accumulate>::new(model);
        drop(reply.take(body));
        reply.finish()
    }
}

#[cfg(test)]
pub(crate) fn request(call: &CompletionRequest<'_>) -> crate::error::Result<Value> {
    body::body(call, &call.reasoning)?.value()
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
        let Some(frame) = crate::provider::wire::parse::<Option<Chunk<'_>>>(frame)? else {
            return Ok(Vec::new());
        };
        if let Some(error) = &frame.error {
            return Err(super::failed(
                error.code.and_then(|code| u16::try_from(code).ok()),
                error.status.as_deref().unwrap_or(""),
                error.message.as_deref().unwrap_or(""),
            ));
        }
        if let Some(reason) = frame
            .prompt_feedback
            .as_ref()
            .and_then(|feedback| feedback.block_reason.as_deref())
        {
            return Err(super::blocked(reason));
        }
        if let Some(reason) = frame.finish_reason() {
            self.done = true;
            ended(reason)?;
        }
        Ok(self.value(&frame))
    }

    fn finished(&self) -> bool {
        self.done
    }

    fn spill(&mut self, partial: &mut Partial) {
        partial.text.push_str(&self.text);
        partial.reasoning.push_str(&self.thinking);
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
    fn value(&mut self, frame: &Chunk<'_>) -> Vec<Delta> {
        let thought = self.thinking.len();
        let named = self.calls.len();
        let text = self.absorb(frame);
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

    #[cfg(test)]
    pub(crate) fn take(&mut self, body: &Value) -> Option<String> {
        let text = body.to_string();
        let chunk = crate::provider::wire::parse::<Chunk<'_>>(&text).unwrap_or_default();
        self.absorb(&chunk)
    }

    fn absorb(&mut self, body: &Chunk<'_>) -> Option<String> {
        if let Some(counted) = &body.usage_metadata {
            self.usage = counted.usage();
        }
        if let Some(reason) = body.finish_reason() {
            self.stop = stop_reason(reason);
        }
        let mut chunk = String::new();
        for part in body.parts() {
            let signature = part.thought_signature.as_deref();
            if let Some(call) = &part.function_call {
                self.calls.push(self.called(call));
                self.signatures
                    .push(signature.unwrap_or_default().to_string());
            } else if let Some(signature) = signature {
                self.said = signature.to_string();
            }
            if let Some(text) = part.text.as_deref() {
                if part.thought == Some(true) {
                    self.thinking.push_str(text);
                } else {
                    self.text.push_str(text);
                    chunk.push_str(text);
                }
            }
        }
        (!chunk.is_empty()).then_some(chunk)
    }

    fn called(&self, call: &Call<'_>) -> ToolCall {
        let name = call.name.as_deref().unwrap_or_default().to_string();
        let id = call.id.as_deref().map_or_else(
            || format!("{SYNTHETIC}{name}-{}", self.calls.len()),
            str::to_string,
        );
        ToolCall {
            id,
            kind: ToolKind::Function,
            function: ToolFunction {
                name,
                arguments: call
                    .args
                    .map_or_else(|| "{}".to_string(), |args| args.get().to_string()),
                checked: Checked::Valid,
            },
            continuation: None,
        }
    }
}

fn ended(reason: &str) -> crate::error::Result<()> {
    if matches!(
        reason,
        "SAFETY" | "RECITATION" | "BLOCKLIST" | "PROHIBITED_CONTENT" | "SPII" | "IMAGE_SAFETY"
    ) {
        return Err(super::blocked(reason));
    }
    if reason == "MALFORMED_FUNCTION_CALL" {
        return Err(crate::error::Error::provider(
            crate::error::ProviderFailureKind::MalformedToolCall,
            format!("native response ended with {reason}"),
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(crate) fn usage(counted: &Value) -> Usage {
    serde_json::from_value::<Counted>(counted.clone())
        .unwrap_or_default()
        .usage()
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
