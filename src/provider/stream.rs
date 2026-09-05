use futures::StreamExt;
use serde::Deserialize;

use super::{
    AssistantMessage, Client, CompletionRequest, StopReason, ToolCall, ToolFunction, ToolKind,
    Usage, think,
};
use crate::error::{Error, ProviderFailure, ProviderFailureKind, Result};

impl Client {
    pub(super) async fn complete_stream(
        &self,
        request: &CompletionRequest,
    ) -> Result<AssistantMessage> {
        let body = self.request_body(request, true)?;
        let mut bytes = self.send(&body).await?.bytes_stream();
        let mut state = Stream::default();
        while let Some(chunk) = bytes.next().await {
            let chunk = chunk.map_err(|error| {
                Error::Provider(ProviderFailure {
                    kind: ProviderFailureKind::Transport,
                    status: None,
                    message: error.to_string(),
                    partial_output: state.has_output(),
                })
            })?;
            state.push(self, &chunk)?;
        }
        state.complete(self)
    }
}

#[derive(Default)]
struct Stream {
    buffer: Vec<u8>,
    text: String,
    reasoning: String,
    think: think::Scan,
    calls: Vec<ToolCall>,
    announced: Vec<bool>,
    usage: Usage,
    stop_reason: StopReason,
    output_started: bool,
    finished: bool,
}

impl Stream {
    fn push(&mut self, client: &Client, bytes: &[u8]) -> Result<()> {
        self.buffer.extend_from_slice(bytes);
        self.consume(client)
    }

    fn consume(&mut self, client: &Client) -> Result<()> {
        while let Some((end, delimiter)) = frame_end(&self.buffer) {
            let bytes: Vec<u8> = self.buffer.drain(..end).collect();
            self.buffer.drain(..delimiter);
            let event = std::str::from_utf8(&bytes).map_err(|error| {
                self.provider_error(
                    ProviderFailureKind::Malformed,
                    format!("invalid SSE UTF-8: {error}"),
                )
            })?;
            for line in event.lines() {
                self.line(client, line)?;
            }
        }
        Ok(())
    }

    fn line(&mut self, client: &Client, line: &str) -> Result<()> {
        let Some(data) = line.strip_prefix("data:").map(str::trim) else {
            return Ok(());
        };
        if data.is_empty() {
            return Ok(());
        }
        if data == "[DONE]" {
            self.finished = true;
            return Ok(());
        }
        let chunk: Chunk = serde_json::from_str(data).map_err(|error| {
            self.provider_error(ProviderFailureKind::Malformed, error.to_string())
        })?;
        if let Some(failure) = chunk.error {
            let message = if failure.message.is_empty() {
                "provider reported an error mid-stream".to_string()
            } else {
                failure.message
            };
            return Err(self.provider_error(ProviderFailureKind::Server, message));
        }
        if let Some(usage) = chunk.usage {
            self.usage = usage;
        }
        if let Some(choice) = chunk.choices.into_iter().next() {
            if let Some(reason) = choice.finish_reason {
                self.stop_reason = reason;
                self.finished = true;
            }
            self.delta(client, choice.delta);
        }
        Ok(())
    }

    fn has_output(&self) -> bool {
        self.output_started
            || !self.text.is_empty()
            || !self.reasoning.is_empty()
            || !self.calls.is_empty()
    }

    fn provider_error(&self, kind: ProviderFailureKind, message: impl Into<String>) -> Error {
        Error::Provider(ProviderFailure {
            kind,
            status: None,
            message: message.into(),
            partial_output: self.has_output(),
        })
    }

    fn delta(&mut self, client: &Client, delta: Delta) {
        self.output_started |= delta
            .reasoning_content
            .as_deref()
            .is_some_and(|part| !part.is_empty())
            || delta
                .content
                .as_deref()
                .is_some_and(|part| !part.is_empty())
            || !delta.tool_calls.is_empty();
        if let Some(part) = delta.reasoning_content.filter(|text| !text.is_empty()) {
            self.push_reason(client, &part);
        }
        if let Some(part) = delta.content.filter(|text| !text.is_empty()) {
            let mut text = String::new();
            let mut thought = String::new();
            self.think.push(
                &part,
                |chunk| text.push_str(chunk),
                |chunk| thought.push_str(chunk),
            );
            self.push_text(client, &text);
            self.push_reason(client, &thought);
        }
        for part in delta.tool_calls {
            merge_call(&mut self.calls, part);
        }
        self.announce(client);
    }

    fn announce(&mut self, client: &Client) {
        self.announced.resize(self.calls.len(), false);
        for (call, announced) in self.calls.iter().zip(self.announced.iter_mut()) {
            if !*announced && !call.function.name.is_empty() {
                *announced = true;
                if let Some(emit) = &client.on_tool {
                    emit(&call.function.name);
                }
            }
        }
    }

    fn push_text(&mut self, client: &Client, part: &str) {
        if part.is_empty() {
            return;
        }
        if let Some(emit) = &client.on_delta {
            emit(part);
        }
        self.text.push_str(part);
    }

    fn push_reason(&mut self, client: &Client, part: &str) {
        if part.is_empty() {
            return;
        }
        if let Some(emit) = &client.on_reasoning {
            emit(part);
        }
        self.reasoning.push_str(part);
    }

    fn flush(&mut self, client: &Client) {
        let mut text = String::new();
        let mut thought = String::new();
        let scan = std::mem::take(&mut self.think);
        scan.finish(
            |chunk| text.push_str(chunk),
            |chunk| thought.push_str(chunk),
        );
        self.push_text(client, &text);
        self.push_reason(client, &thought);
    }

    fn complete(mut self, client: &Client) -> Result<AssistantMessage> {
        if !self.finished {
            return Err(self.provider_error(
                ProviderFailureKind::Malformed,
                "stream ended before a finish reason or [DONE] marker",
            ));
        }
        if self.buffer.iter().any(|byte| !byte.is_ascii_whitespace()) {
            return Err(self.provider_error(
                ProviderFailureKind::Malformed,
                "stream ended with an incomplete SSE frame",
            ));
        }
        self.flush(client);
        let partial = self.has_output();
        finish(self.message()).map_err(|error| match error {
            Error::Provider(mut failure) => {
                failure.partial_output |= partial;
                Error::Provider(failure)
            }
            error => error,
        })
    }

    fn message(self) -> AssistantMessage {
        AssistantMessage {
            content: (!self.text.is_empty()).then_some(self.text),
            tool_calls: self.calls,
            reasoning_content: (!self.reasoning.is_empty()).then_some(self.reasoning),
            usage: self.usage,
            stop_reason: self.stop_reason,
        }
    }
}

fn frame_end(bytes: &[u8]) -> Option<(usize, usize)> {
    let lf = bytes.windows(2).position(|window| window == b"\n\n");
    let crlf = bytes.windows(4).position(|window| window == b"\r\n\r\n");
    match (lf, crlf) {
        (Some(left), Some(right)) if left <= right => Some((left, 2)),
        (Some(_), Some(right)) => Some((right, 4)),
        (Some(left), None) => Some((left, 2)),
        (None, Some(right)) => Some((right, 4)),
        (None, None) => None,
    }
}

pub(super) fn merge_call(calls: &mut Vec<ToolCall>, part: CallDelta) {
    let index = part.index.unwrap_or(calls.len().saturating_sub(1));
    if index > calls.len() {
        return;
    }
    if calls.len() == index {
        calls.push(ToolCall {
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
    let empty = message
        .content
        .as_deref()
        .is_none_or(|text| text.trim().is_empty());
    let truncated_reasoning = message.stop_reason == StopReason::Length
        && message
            .reasoning_content
            .as_deref()
            .is_some_and(|text| !text.trim().is_empty());
    if empty && message.tool_calls.is_empty() && !truncated_reasoning {
        return Err(Error::provider(
            ProviderFailureKind::Malformed,
            "stream produced neither text nor a tool call",
        ));
    }
    Ok(message)
}

#[derive(Deserialize)]
pub(super) struct Completion {
    #[serde(default)]
    pub(super) choices: Vec<Choice>,
    pub(super) usage: Option<Usage>,
}

#[derive(Deserialize)]
pub(super) struct Choice {
    pub(super) message: AssistantMessage,
    pub(super) finish_reason: Option<StopReason>,
}

#[derive(Deserialize)]
struct Chunk {
    #[serde(default)]
    choices: Vec<ChunkChoice>,
    usage: Option<Usage>,
    error: Option<StreamError>,
}

#[derive(Deserialize)]
struct StreamError {
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
struct ChunkChoice {
    #[serde(default)]
    delta: Delta,
    finish_reason: Option<StopReason>,
}

#[derive(Default, Deserialize)]
struct Delta {
    content: Option<String>,
    #[serde(
        default,
        alias = "reasoning",
        deserialize_with = "think::reasoning_field"
    )]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Vec<CallDelta>,
}

#[derive(Deserialize)]
pub(super) struct CallDelta {
    pub(super) index: Option<usize>,
    pub(super) id: Option<String>,
    pub(super) function: Option<FunctionDelta>,
}

#[derive(Deserialize)]
pub(super) struct FunctionDelta {
    pub(super) name: Option<String>,
    pub(super) arguments: Option<String>,
}

#[cfg(test)]
#[path = "stream_tests.rs"]
mod tests;
