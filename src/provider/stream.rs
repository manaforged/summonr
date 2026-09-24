use futures::StreamExt;
use serde::Deserialize;

use super::{
    AssistantMessage, Client, CompletionRequest, Partial, StopReason, ToolCall, ToolFunction,
    ToolKind, think,
};
use crate::error::{Error, ProviderFailureKind, Result};

impl Client {
    pub(super) async fn complete_stream(
        &self,
        request: &CompletionRequest<'_>,
        partial: &mut Partial,
    ) -> Result<AssistantMessage> {
        match self.wire {
            crate::endpoint::Wire::Anthropic => {
                self.stream_wire::<super::wire::anthropic::Wire>(request, partial)
                    .await
            }
            crate::endpoint::Wire::Gemini => {
                self.stream_wire::<super::wire::gemini::Wire>(request, partial)
                    .await
            }
            crate::endpoint::Wire::Responses => {
                self.stream_wire::<super::wire::responses::Wire>(request, partial)
                    .await
            }
            crate::endpoint::Wire::Completions | crate::endpoint::Wire::Decisions => {
                self.stream_wire::<super::wire::completions::Wire>(request, partial)
                    .await
            }
        }
    }

    async fn stream_wire<W: super::wire::Chat>(
        &self,
        request: &CompletionRequest<'_>,
        partial: &mut Partial,
    ) -> Result<AssistantMessage> {
        use super::wire::{Accumulate, Delta};

        let body = self.request_body(request, true)?;
        let url = self.url_for::<W>(&request.model, true);
        let sink = super::capture::Sink::open(self.wire, "stream");
        if let Some(sink) = &sink {
            sink.write("request", &body.to_string());
        }
        let mut bytes = self
            .send(&url, &body, &request.session, true)
            .await?
            .into_stream()
            .map_err(|error| Error::provider(ProviderFailureKind::Transport, error.to_string()))?;
        let mut frames = super::wire::Frames::default();
        let mut state = W::Stream::new(&request.model);
        let cut = |error: Error| match error {
            Error::Provider(mut failure) => {
                failure.partial_output = true;
                Error::Provider(failure)
            }
            error => error,
        };
        while let Some(chunk) = bytes.next().await {
            let chunk = chunk.map_err(|error| {
                cut(Error::provider(
                    ProviderFailureKind::Transport,
                    error.to_string(),
                ))
            })?;
            if let Some(sink) = &sink {
                sink.write("chunk", &String::from_utf8_lossy(&chunk));
            }
            for frame in frames.push(&chunk, W::sentinel()).map_err(cut)? {
                for delta in state.on(&frame).map_err(cut)? {
                    match delta {
                        Delta::Text(text) => {
                            if request.emit_deltas
                                && let Some(emit) = &self.on_delta
                            {
                                emit(&text);
                            }
                            partial.text.push_str(&text);
                        }
                        Delta::Thinking(text) => {
                            if request.emit_deltas
                                && let Some(emit) = &self.on_reasoning
                            {
                                emit(&text);
                            }
                            partial.reasoning.push_str(&text);
                        }
                        Delta::Call(name) => {
                            if request.emit_deltas
                                && let Some(emit) = &self.on_tool
                            {
                                emit(&name);
                            }
                        }
                    }
                    if partial.text.len() > PARTIAL_CAP || partial.reasoning.len() > PARTIAL_CAP {
                        return Err(cut(Error::provider(
                            ProviderFailureKind::Malformed,
                            format!("streamed output exceeded {PARTIAL_CAP} bytes"),
                        )));
                    }
                }
            }
            if frames.ended() {
                break;
            }
        }
        if !frames.ended() && !state.finished() {
            return Err(cut(Error::provider(
                ProviderFailureKind::Transport,
                "stream ended before the provider finished the reply",
            )));
        }
        finish(state.finish()).map_err(cut)
    }
}

pub(crate) const PARTIAL_CAP: usize = 8 * 1024 * 1024;
pub(crate) const ARGUMENT_CAP: usize = 1024 * 1024;

pub(crate) fn merge_call(calls: &mut Vec<ToolCall>, part: CallDelta) -> Result<()> {
    let index = part.index.unwrap_or(calls.len().saturating_sub(1));
    if index > calls.len() {
        return Ok(());
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
            if call.function.arguments.len() > ARGUMENT_CAP {
                return Err(Error::provider(
                    ProviderFailureKind::Malformed,
                    format!("streamed tool arguments exceeded {ARGUMENT_CAP} bytes"),
                ));
            }
        }
    }
    Ok(())
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
