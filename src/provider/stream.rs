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
        use super::wire::Accumulate;

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
        let mut heard = std::time::Instant::now();
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
            let parsed = frames.push(&chunk, W::sentinel()).map_err(cut)?;
            let produced = self
                .take(&mut state, &parsed, request, partial)
                .map_err(cut)?;
            self.heard(&mut heard, produced == 0).map_err(cut)?;
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

    fn take<S: super::wire::Accumulate>(
        &self,
        state: &mut S,
        frames: &[String],
        request: &CompletionRequest<'_>,
        partial: &mut Partial,
    ) -> Result<usize> {
        let mut produced = 0;
        for frame in frames {
            for delta in state.on(frame)? {
                self.apply(request, partial, delta)?;
                produced += 1;
            }
        }
        Ok(produced)
    }

    fn heard(&self, since: &mut std::time::Instant, silent: bool) -> Result<()> {
        if !silent {
            *since = std::time::Instant::now();
            return Ok(());
        }
        let Some(idle) = self.idle.filter(|idle| since.elapsed() > *idle) else {
            return Ok(());
        };
        Err(Error::provider(
            ProviderFailureKind::Transport,
            format!("stream produced no output for {} s", idle.as_secs()),
        ))
    }

    fn apply(
        &self,
        request: &CompletionRequest<'_>,
        partial: &mut Partial,
        delta: super::wire::Delta,
    ) -> Result<()> {
        use super::wire::Delta;

        let (emit, sink) = match delta {
            Delta::Text(text) => (
                self.on_delta.as_ref().map(|emit| (emit, text.clone())),
                Some((&mut partial.text, text)),
            ),
            Delta::Thinking(text) => (
                self.on_reasoning.as_ref().map(|emit| (emit, text.clone())),
                Some((&mut partial.reasoning, text)),
            ),
            Delta::Call(name) => (self.on_tool.as_ref().map(|emit| (emit, name)), None),
        };
        if request.emit_deltas
            && let Some((emit, text)) = emit
        {
            emit(&text);
        }
        if let Some((buffer, text)) = sink {
            buffer.push_str(&text);
        }
        if partial.text.len() > PARTIAL_CAP || partial.reasoning.len() > PARTIAL_CAP {
            return Err(Error::provider(
                ProviderFailureKind::Malformed,
                format!("streamed output exceeded {PARTIAL_CAP} bytes"),
            ));
        }
        Ok(())
    }
}

fn cut(error: Error) -> Error {
    match error {
        Error::Provider(mut failure) => {
            failure.partial_output = true;
            Error::Provider(failure)
        }
        error => error,
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
