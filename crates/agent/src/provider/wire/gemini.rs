#[cfg(test)]
use serde_json::Value;

use super::reply::ReplyBuilder;
use super::{Accumulate, Chat, Delta};
#[cfg(test)]
use crate::provider::Usage;
use crate::provider::{
    AssistantMessage, CompletionRequest, Continuation, StopReason, ToolCall, ToolFunction, ToolKind,
};
#[cfg(test)]
use frame::Counted;
use frame::{Call, Chunk};
use summonr_types::Checked;

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

    fn body(
        call: &CompletionRequest<'_>,
        encoding: &super::Encoding<'_>,
    ) -> summonr_core::error::Result<super::Wired> {
        body::body(call, encoding.reasoning)
    }
}

#[cfg(test)]
pub(crate) fn request(call: &CompletionRequest<'_>) -> summonr_core::error::Result<Value> {
    body::body(call, &call.reasoning)?.value()
}

#[derive(Default)]
pub(crate) struct Reply {
    pub(crate) model: String,
    reply: ReplyBuilder,
    pub(crate) signatures: Vec<String>,
    pub(crate) said: String,
}

impl Accumulate for Reply {
    fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            ..Self::default()
        }
    }

    fn on(&mut self, frame: &str) -> summonr_core::error::Result<Vec<Delta>> {
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
            self.reply.finished = true;
            crate::provider::client::ending(crate::endpoint::Wire::Gemini, reason, "")?;
        }
        Ok(self.value(&frame))
    }

    fn reply(&self) -> &ReplyBuilder {
        &self.reply
    }

    fn finish(mut self) -> AssistantMessage {
        if !self.reply.calls.is_empty() {
            self.reply.stop = StopReason::ToolCalls;
            let mut signatures = std::mem::take(&mut self.signatures);
            if !self.said.is_empty() {
                signatures.push(std::mem::take(&mut self.said));
            }
            if signatures.iter().any(|signature| !signature.is_empty())
                && let Some(call) = self.reply.calls.first_mut()
            {
                call.continuation = Some(Continuation::Gemini {
                    model: self.model.clone(),
                    signatures,
                });
            }
        }
        let continuation =
            (self.reply.calls.is_empty() && !self.said.is_empty()).then(|| Continuation::Gemini {
                model: self.model.clone(),
                signatures: vec![std::mem::take(&mut self.said)],
            });
        self.reply.finish(continuation)
    }
}

impl Reply {
    fn value(&mut self, frame: &Chunk<'_>) -> Vec<Delta> {
        let thought = self.reply.reasoning.len();
        let named = self.reply.calls.len();
        let text = self.absorb(frame);
        let mut deltas = Vec::new();
        if self.reply.reasoning.len() > thought {
            deltas.push(Delta::Thinking(self.reply.reasoning[thought..].to_string()));
        }
        if let Some(chunk) = text {
            deltas.push(Delta::Text(chunk));
        }
        for call in self.reply.calls.iter().skip(named) {
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
            self.reply.usage = counted.usage();
        }
        if let Some(reason) = body.finish_reason() {
            self.reply.stop = stop_reason(reason);
        }
        let mut chunk = String::new();
        for part in body.parts() {
            let signature = part.thought_signature.as_deref();
            if let Some(call) = &part.function_call {
                let called = self.called(call);
                self.reply.calls.push(called);
                self.signatures
                    .push(signature.unwrap_or_default().to_string());
            } else if let Some(signature) = signature {
                self.said = signature.to_string();
            }
            if let Some(text) = part.text.as_deref() {
                if part.thought == Some(true) {
                    self.reply.reasoning.push_str(text);
                } else {
                    self.reply.text.push_str(text);
                    chunk.push_str(text);
                }
            }
        }
        (!chunk.is_empty()).then_some(chunk)
    }

    fn called(&self, call: &Call<'_>) -> ToolCall {
        let name = call.name.as_deref().unwrap_or_default().to_string();
        let id = call.id.as_deref().map_or_else(
            || format!("{SYNTHETIC}{name}-{}", self.reply.calls.len()),
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

#[cfg(test)]
pub(crate) fn usage(counted: &Value) -> Usage {
    serde_json::from_value::<Counted>(counted.clone())
        .unwrap_or_default()
        .usage()
}

pub(crate) fn stop_reason(raw: &str) -> StopReason {
    crate::provider::client::ending(crate::endpoint::Wire::Gemini, raw, "")
        .ok()
        .flatten()
        .unwrap_or(StopReason::Other)
}

#[cfg(test)]
#[path = "gemini_tests.rs"]
mod tests;
