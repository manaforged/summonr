use std::mem::{replace, take};
use std::sync::{Arc, Mutex};

use base64::{Engine, engine::general_purpose::STANDARD};
use futures::StreamExt;
use genai::adapter::AdapterKind;
use genai::chat::{
    CacheControl, ChatMessage, ChatOptions, ChatRequest, ChatRole, ChatStreamEvent, ContentPart,
    MessageContent, MessageOptions, Tool, ToolCall as GenaiCall, ToolName, ToolResponse,
};
use genai::resolver::{AuthData, Endpoint as GenaiEndpoint, ProviderConfig};
use genai::{ModelIden, ServiceTarget};

use crate::endpoint::Wire;
use crate::error::{Error, ProviderFailure, ProviderFailureKind, Result};
use crate::provider::{
    AssistantMessage, CompletionRequest, Continuation, Partial, Role, StopReason, ToolCall,
    ToolFunction, ToolKind, Usage, response_failure,
};

use super::Client;
use super::stream::validate;

mod anthropic;

impl Client {
    pub(super) fn native_kind(wire: Wire) -> Option<AdapterKind> {
        match wire {
            Wire::Responses => Some(AdapterKind::OpenAIResp),
            Wire::Anthropic => Some(AdapterKind::Anthropic),
            Wire::Gemini => Some(AdapterKind::Gemini),
            Wire::Completions | Wire::Decisions => None,
        }
    }

    pub(super) async fn complete_native(
        &self,
        request: &CompletionRequest,
        kind: AdapterKind,
        output: &mut Partial,
    ) -> Result<AssistantMessage> {
        let genai = self
            .genai
            .as_ref()
            .ok_or_else(|| Error::Config("native wire is missing a genai client".to_string()))?;
        let token = self.token().await?;
        let target = ServiceTarget {
            endpoint: GenaiEndpoint::from_owned(self.native_base()),
            auth: AuthData::from_single(token),
            model: ModelIden::new(kind, request.model.clone()),
        };
        let chat = to_chat(request, self.wire)?;
        let options = chat_options(request.max_output_tokens, request.effort)
            .with_extra_headers(self.headers(Some(&request.session)))
            .with_capture_raw_body(
                matches!(self.wire, Wire::Anthropic | Wire::Responses)
                    || super::capture::directory().is_some(),
            );
        if request.emit_deltas && (self.on_delta.is_some() || self.on_reasoning.is_some()) {
            return self
                .stream_native(genai, target, chat, &options, output)
                .await;
        }
        let response = genai
            .exec_chat(target, chat, Some(&options))
            .await
            .map_err(map_genai)?;
        if let Some(sink) = super::capture::Sink::open(self.wire, "reply")
            && let Some(raw) = response.captured_raw_body.as_ref()
        {
            sink.write("body", &raw.to_string());
        }
        let blocks = if self.wire == Wire::Anthropic {
            Some(anthropic::blocks(response.captured_raw_body.as_ref())?)
        } else {
            None
        };
        let mut message = from_response(response, &request.model, self.wire)?;
        anthropic::retain(&mut message, &request.model, blocks);
        Ok(message)
    }

    async fn stream_native(
        &self,
        genai: &genai::Client,
        target: ServiceTarget,
        chat: ChatRequest,
        options: &ChatOptions,
        output: &mut Partial,
    ) -> Result<AssistantMessage> {
        let model = target.model.model_name.to_string();
        let capture = (self.wire == Wire::Anthropic)
            .then(|| Arc::new(Mutex::new(Ok(anthropic::Capture::default()))));
        let sink = super::capture::Sink::open(self.wire, "stream");
        let options = if capture.is_some() || sink.is_some() {
            let capture = capture.clone();
            let sink = sink.clone();
            options.clone().with_raw_frame_fn(move |_, frame| {
                if let Some(sink) = &sink {
                    sink.write("frame", frame.data);
                }
                if let Some(capture) = &capture
                    && let Ok(mut state) = capture.lock()
                    && let Ok(blocks) = &mut *state
                    && let Err(error) = blocks.push(frame)
                {
                    let failed = Err(error);
                    *state = failed;
                }
            })
        } else {
            options.clone()
        };
        let streamed = genai
            .exec_chat_stream(target, chat, Some(&options))
            .await
            .map_err(map_genai)?;
        let content = &mut output.text;
        let reasoning = &mut output.reasoning;
        let mut tool_calls = Vec::new();
        let mut usage = Usage::default();
        let mut stop = None;
        let mut output_started = false;
        let mut stream = streamed.stream;
        while let Some(event) = stream.next().await {
            let event = event.map_err(|error| native_error(error, output_started))?;
            match event {
                ChatStreamEvent::Chunk(chunk) => {
                    output_started |= !chunk.content.is_empty();
                    if let Some(emit) = &self.on_delta {
                        emit(&chunk.content);
                    }
                    content.push_str(&chunk.content);
                }
                ChatStreamEvent::ReasoningChunk(chunk) => {
                    output_started |= !chunk.content.is_empty();
                    if let Some(emit) = &self.on_reasoning {
                        emit(&chunk.content);
                    }
                    reasoning.push_str(&chunk.content);
                }
                ChatStreamEvent::ToolCallChunk(_) => {
                    output_started = true;
                }
                ChatStreamEvent::End(end) => {
                    if let Some(captured) = end.captured_content {
                        if content.is_empty()
                            && let Some(text) = captured.first_text()
                        {
                            *content = text.to_string();
                        }
                        tool_calls = captured
                            .into_tool_calls()
                            .into_iter()
                            .map(|call| from_genai_call(call, &model, self.wire))
                            .collect();
                    }
                    if reasoning.is_empty()
                        && let Some(text) = end.captured_reasoning_content
                    {
                        *reasoning = text;
                    }
                    if let Some(genai_usage) = end.captured_usage {
                        usage = from_genai_usage(&genai_usage);
                    }
                    if let Some(reason) = end.captured_stop_reason {
                        stop = Some(reason);
                    }
                }
                ChatStreamEvent::Start
                | ChatStreamEvent::Heartbeat
                | ChatStreamEvent::ThoughtSignatureChunk(_) => {}
            }
        }
        let Some(stop) = stop else {
            return Err(Error::Provider(ProviderFailure {
                kind: ProviderFailureKind::Malformed,
                status: None,
                message: "native stream ended without a terminal stop reason".to_string(),
                partial_output: output_started,
                retry_after: None,
            }));
        };
        let stop = from_genai_stop(&stop, output_started)?;
        let blocks = if let Some(capture) = capture {
            let mut state = capture.lock().map_err(|error| {
                partial(
                    Error::provider(
                        ProviderFailureKind::Malformed,
                        format!("native content capture failed: {error}"),
                    ),
                    output_started,
                )
            })?;
            let blocks = replace(&mut *state, Ok(anthropic::Capture::default()))
                .and_then(anthropic::Capture::finish)
                .map_err(|error| partial(error, output_started))?;
            Some(blocks)
        } else {
            None
        };
        if stop != StopReason::Length {
            validate(content, reasoning, &tool_calls, stop)
                .map_err(|error| partial(error, output_started))?;
        }
        let mut message = AssistantMessage {
            content: (!content.is_empty()).then(|| take(content)),
            tool_calls,
            reasoning_content: (!reasoning.is_empty()).then(|| take(reasoning)),
            usage,
            stop_reason: stop,
        };
        anthropic::retain(&mut message, &model, blocks);
        Ok(message)
    }

    pub(super) async fn list_native(&self, kind: AdapterKind) -> Result<Vec<String>> {
        let genai = self
            .genai
            .as_ref()
            .ok_or_else(|| Error::Config("native wire is missing a genai client".to_string()))?;
        let token = self.token().await?;
        let listed = genai
            .all_model_names(
                kind,
                ProviderConfig::default()
                    .with_endpoint(GenaiEndpoint::from_owned(self.native_base()))
                    .with_auth(AuthData::from_single(token)),
            )
            .await
            .map_err(map_genai)?;
        if listed.is_empty() {
            Ok(crate::endpoint::catalog(self.wire))
        } else {
            Ok(listed)
        }
    }

    fn native_base(&self) -> String {
        let base = self
            .url
            .strip_suffix("/chat/completions")
            .unwrap_or(&self.url);
        if base.ends_with('/') {
            base.to_string()
        } else {
            format!("{base}/")
        }
    }
}

fn chat_options(max_tokens: Option<u32>, effort: Option<crate::provider::Effort>) -> ChatOptions {
    use crate::provider::Effort;
    use genai::chat::ReasoningEffort;
    let mut options = ChatOptions::default()
        .with_capture_content(true)
        .with_capture_tool_calls(true)
        .with_capture_usage(true)
        .with_capture_reasoning_content(true)
        .with_normalize_reasoning_content(true);
    if let Some(max) = max_tokens {
        options = options.with_max_tokens(max);
    }
    if let Some(effort) = effort.filter(|effort| *effort != Effort::None) {
        options = options.with_reasoning_effort(match effort {
            Effort::None => ReasoningEffort::Zero,
            Effort::Low => ReasoningEffort::Low,
            Effort::Medium => ReasoningEffort::Medium,
            Effort::High => ReasoningEffort::High,
        });
    }
    options
}

fn to_chat(request: &CompletionRequest, wire: Wire) -> Result<ChatRequest> {
    let mut system = String::new();
    let mut messages = Vec::new();
    for message in &request.messages {
        match message.role {
            Role::System if messages.is_empty() => {
                if !system.is_empty() {
                    system.push('\n');
                }
                system.push_str(&message.content);
            }
            Role::User | Role::System => {
                let mut parts = Vec::with_capacity(message.images.len() + 1);
                if !message.content.is_empty() {
                    parts.push(ContentPart::Text(message.content.clone()));
                }
                parts.extend(message.images.iter().map(|image| {
                    ContentPart::from_binary_base64(
                        &image.media_type,
                        STANDARD.encode(&image.bytes),
                        None,
                    )
                }));
                messages.push(ChatMessage::user(MessageContent::from_parts(parts)));
            }
            Role::Assistant => {
                if wire == Wire::Anthropic
                    && let Some(blocks) =
                        message
                            .tool_calls
                            .iter()
                            .find_map(|call| match &call.continuation {
                                Some(Continuation::Anthropic { model, blocks })
                                    if model == &request.model =>
                                {
                                    Some(blocks)
                                }
                                _ => None,
                            })
                {
                    messages.push(ChatMessage::assistant(MessageContent::from_parts(
                        blocks
                            .iter()
                            .cloned()
                            .map(|block| ContentPart::from_custom(block, None))
                            .collect::<Vec<_>>(),
                    )));
                    continue;
                }
                let mut parts = Vec::new();
                if !message.content.is_empty() {
                    parts.push(ContentPart::Text(message.content.clone()));
                }
                for call in &message.tool_calls {
                    let signature = match (&call.continuation, wire) {
                        (Some(Continuation::Gemini { model, signatures }), Wire::Gemini)
                        | (Some(Continuation::Responses { model, signatures }), Wire::Responses)
                            if model == &request.model =>
                        {
                            Some(signatures)
                        }
                        _ => None,
                    };
                    if let Some(signatures) = signature {
                        parts.extend(
                            signatures
                                .iter()
                                .cloned()
                                .map(ContentPart::ThoughtSignature),
                        );
                    }
                    parts.push(ContentPart::ToolCall(to_genai_call(call)?));
                }
                messages.push(ChatMessage {
                    role: ChatRole::Assistant,
                    content: MessageContent::from_parts(parts),
                    options: None,
                });
            }
            Role::Tool => {
                let id = message.tool_call_id.clone().unwrap_or_default();
                messages.push(ChatMessage {
                    role: ChatRole::Tool,
                    content: MessageContent::from_tool_responses(vec![ToolResponse::new(
                        id,
                        message.content.clone(),
                    )]),
                    options: None,
                });
            }
        }
    }
    if let Some(last) = messages.last_mut() {
        last.options = Some(MessageOptions::from(CacheControl::Ephemeral));
    }
    let tools = if request.tools.is_empty() {
        None
    } else {
        Some(
            request
                .tools
                .iter()
                .map(|tool| {
                    Tool::new(ToolName::Custom(tool.function.name.clone()))
                        .with_description(tool.function.description.clone())
                        .with_schema(tool.function.parameters.clone())
                })
                .collect(),
        )
    };
    Ok(ChatRequest {
        system: (!system.is_empty()).then_some(system),
        messages,
        tools,
        previous_response_id: None,
        store: None,
    })
}

fn to_genai_call(call: &ToolCall) -> Result<GenaiCall> {
    let fn_arguments = serde_json::from_str(&call.function.arguments).map_err(|error| {
        Error::provider(
            ProviderFailureKind::MalformedToolCall,
            format!("{}: invalid tool arguments: {error}", call.function.name),
        )
    })?;
    Ok(GenaiCall {
        call_id: call.id.clone(),
        fn_name: call.function.name.clone(),
        fn_arguments,
        thought_signatures: None,
    })
}

fn from_genai_call(call: GenaiCall, model: &str, wire: Wire) -> ToolCall {
    ToolCall {
        id: call.call_id,
        kind: ToolKind::Function,
        function: ToolFunction {
            name: call.fn_name,
            arguments: call.fn_arguments.to_string(),
        },
        continuation: call.thought_signatures.and_then(|signatures| match wire {
            Wire::Gemini => Some(Continuation::Gemini {
                model: model.to_string(),
                signatures,
            }),
            Wire::Responses => Some(Continuation::Responses {
                model: model.to_string(),
                signatures,
            }),
            _ => None,
        }),
    }
}

fn from_response(
    response: genai::chat::ChatResponse,
    model: &str,
    wire: Wire,
) -> Result<AssistantMessage> {
    let content = response.first_text().map(str::to_string);
    let reasoning_content = response.reasoning_content.clone();
    let usage = from_genai_usage(&response.usage);
    let stop_reason = response.stop_reason.as_ref().ok_or_else(|| {
        Error::provider(
            ProviderFailureKind::Malformed,
            "native response has no terminal stop reason",
        )
    })?;
    let stop_reason = from_genai_stop(
        stop_reason,
        content.is_some() || !response.tool_calls().is_empty(),
    )?;
    let signatures: Vec<String> = if wire == Wire::Responses {
        response
            .captured_raw_body
            .as_ref()
            .and_then(|body| body.get("output"))
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter(|item| item["type"] == "reasoning")
            .filter_map(|item| {
                item.get("encrypted_content")
                    .and_then(serde_json::Value::as_str)
            })
            .map(str::to_string)
            .collect()
    } else {
        Vec::new()
    };
    let mut tool_calls: Vec<_> = response
        .into_tool_calls()
        .into_iter()
        .map(|call| from_genai_call(call, model, wire))
        .collect();
    if !signatures.is_empty()
        && let Some(call) = tool_calls.first_mut()
    {
        call.continuation = Some(Continuation::Responses {
            model: model.to_string(),
            signatures,
        });
    }
    Ok(AssistantMessage {
        content,
        tool_calls,
        reasoning_content,
        usage,
        stop_reason,
    })
}

fn from_genai_usage(usage: &genai::chat::Usage) -> Usage {
    Usage {
        prompt_tokens: usage.prompt_tokens.unwrap_or_default().max(0) as u32,
        completion_tokens: usage.completion_tokens.unwrap_or_default().max(0) as u32,
        reasoning_tokens: usage
            .completion_tokens_details
            .as_ref()
            .and_then(|details| details.reasoning_tokens)
            .unwrap_or_default()
            .max(0) as u32,
        cached_tokens: usage
            .prompt_tokens_details
            .as_ref()
            .and_then(|details| details.cached_tokens)
            .unwrap_or_default()
            .max(0) as u32,
    }
}

fn from_genai_stop(reason: &genai::chat::StopReason, partial_output: bool) -> Result<StopReason> {
    match reason {
        genai::chat::StopReason::ToolCall(_) => Ok(StopReason::ToolCalls),
        genai::chat::StopReason::MaxTokens(_) => Ok(StopReason::Length),
        genai::chat::StopReason::Completed(_) | genai::chat::StopReason::StopSequence(_) => {
            Ok(StopReason::Stop)
        }
        genai::chat::StopReason::ContentFilter(reason) => Err(Error::Provider(ProviderFailure {
            kind: ProviderFailureKind::Http,
            status: None,
            message: format!("native response was blocked by the provider: {reason}"),
            partial_output,
            retry_after: None,
        })),
        genai::chat::StopReason::Other(reason) => Err(Error::Provider(ProviderFailure {
            kind: ProviderFailureKind::Malformed,
            status: None,
            message: format!("native response ended with an unsupported stop reason: {reason}"),
            partial_output,
            retry_after: None,
        })),
    }
}

pub(super) fn map_genai(error: genai::Error) -> Error {
    let text = error.to_string();
    let kind = match error {
        genai::Error::RequiresApiKey { .. }
        | genai::Error::NoAuthResolver { .. }
        | genai::Error::NoAuthData { .. } => ProviderFailureKind::Authentication,
        genai::Error::StreamParse { .. }
        | genai::Error::ChatResponseGeneration { .. }
        | genai::Error::InvalidJsonResponseElement { .. }
        | genai::Error::NoChatResponse { .. }
        | genai::Error::SerdeJson(_)
        | genai::Error::JsonValueExt(_) => ProviderFailureKind::Malformed,
        genai::Error::ChatResponse { .. } => ProviderFailureKind::Server,
        genai::Error::WebStream { error, .. } => match error.downcast::<genai::Error>() {
            Ok(error) => return map_genai(*error),
            Err(_transport) => ProviderFailureKind::Transport,
        },
        genai::Error::WebAdapterCall { webc_error, .. }
        | genai::Error::WebModelCall { webc_error, .. } => match webc_error {
            genai::webc::Error::ResponseFailedStatus { status, body, .. } => {
                return response_failure(status.as_u16(), &body, None);
            }
            genai::webc::Error::Reqwest(_) => ProviderFailureKind::Transport,
            _ => ProviderFailureKind::Malformed,
        },
        genai::Error::HttpError { status, body, .. } => {
            return response_failure(status.as_u16(), &body, None);
        }
        _ => ProviderFailureKind::Http,
    };
    Error::provider(kind, text)
}

fn native_error(error: genai::Error, partial_output: bool) -> Error {
    partial(map_genai(error), partial_output)
}

fn partial(error: Error, partial_output: bool) -> Error {
    match error {
        Error::Provider(mut failure) => {
            failure.partial_output = partial_output;
            Error::Provider(failure)
        }
        error => error,
    }
}
