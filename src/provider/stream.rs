use futures::StreamExt;
use serde::Deserialize;

use super::{
    AssistantMessage, Client, CompletionRequest, Partial, StopReason, ToolCall, ToolFunction,
    ToolKind, think,
};
use crate::error::{Error, ProviderFailure, ProviderFailureKind, Result};

impl Client {
    pub(super) async fn complete_stream(
        &self,
        request: &CompletionRequest,
        partial: &mut Partial,
    ) -> Result<AssistantMessage> {
        self.stream_wire::<super::wire::completions::Wire>(request, partial)
            .await
    }

    async fn stream_wire<W: super::wire::Chat>(
        &self,
        request: &CompletionRequest,
        partial: &mut Partial,
    ) -> Result<AssistantMessage> {
        use super::wire::{Accumulate, Delta};

        let body = self.request_body(request, true)?;
        let mut bytes = self
            .send(&body, &request.session)
            .await?
            .into_stream()
            .map_err(|error| Error::provider(ProviderFailureKind::Transport, error.to_string()))?;
        let mut frames = super::wire::Frames::default();
        let mut state = W::Stream::new(&request.model);
        let mut started = false;
        while let Some(chunk) = bytes.next().await {
            let chunk = chunk.map_err(|error| {
                Error::Provider(ProviderFailure {
                    kind: ProviderFailureKind::Transport,
                    status: None,
                    message: error.to_string(),
                    partial_output: started
                        || !partial.text.is_empty()
                        || !partial.reasoning.is_empty(),
                    retry_after: None,
                })
            })?;
            for frame in frames.push(&chunk, W::sentinel())? {
                for delta in state.on(&frame)? {
                    started = true;
                    match delta {
                        Delta::Text(text) => {
                            if let Some(emit) = &self.on_delta {
                                emit(&text);
                            }
                            partial.text.push_str(&text);
                        }
                        Delta::Thinking(text) => {
                            if let Some(emit) = &self.on_reasoning {
                                emit(&text);
                            }
                            partial.reasoning.push_str(&text);
                        }
                        Delta::Call(name) => {
                            if let Some(emit) = &self.on_tool {
                                emit(&name);
                            }
                        }
                    }
                }
            }
            if frames.ended() {
                break;
            }
        }
        finish(state.finish())
    }
}

pub(crate) fn merge_call(calls: &mut Vec<ToolCall>, part: CallDelta) {
    let index = part.index.unwrap_or(calls.len().saturating_sub(1));
    if index > calls.len() {
        return;
    }
    if calls.len() == index {
        calls.push(ToolCall {
            continuation: None,
            id: String::new(),
            kind: ToolKind::Function,
            function: ToolFunction {
                name: String::new(),
                arguments: String::new(),
            },
        });
    }
    let call = &mut calls[index];
    if let Some(id) = part.id.filter(|id| !id.is_empty()) {
        call.id = id;
    }
    if let Some(function) = part.function {
        if let Some(name) = function.name.filter(|name| !name.is_empty()) {
            call.function.name = name;
        }
        if let Some(arguments) = function.arguments {
            call.function.arguments.push_str(&arguments);
        }
    }
}

pub(super) fn finish(mut message: AssistantMessage) -> Result<AssistantMessage> {
    think::hoist(&mut message);
    validate(
        message.content.as_deref().unwrap_or_default(),
        message.reasoning_content.as_deref().unwrap_or_default(),
        &message.tool_calls,
        message.stop_reason,
    )?;
    Ok(message)
}

pub(super) fn validate(
    text: &str,
    reasoning: &str,
    calls: &[ToolCall],
    stop: StopReason,
) -> Result<()> {
    let truncated_reasoning = stop == StopReason::Length && !reasoning.trim().is_empty();
    if text.trim().is_empty() && calls.is_empty() && !truncated_reasoning {
        return Err(Error::provider(
            ProviderFailureKind::Malformed,
            "stream produced neither text nor a tool call",
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
pub(crate) struct CallDelta {
    pub(crate) index: Option<usize>,
    pub(crate) id: Option<String>,
    pub(crate) function: Option<FunctionDelta>,
}

#[derive(Deserialize)]
pub(crate) struct FunctionDelta {
    pub(crate) name: Option<String>,
    pub(crate) arguments: Option<String>,
}
