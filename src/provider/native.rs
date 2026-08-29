use futures::StreamExt;
use genai::adapter::AdapterKind;
use genai::chat::{
    CacheControl, ChatMessage, ChatOptions, ChatRequest, ChatRole, ChatStreamEvent, MessageContent,
    MessageOptions, Tool, ToolCall as GenaiCall, ToolName, ToolResponse,
};
use genai::resolver::{AuthData, Endpoint as GenaiEndpoint, ProviderConfig};
use genai::{ModelIden, ServiceTarget};

use crate::endpoint::Wire;
use crate::error::{Error, ProviderFailureKind, Result};
use crate::provider::{
    AssistantMessage, CompletionRequest, Role, StopReason, ToolCall, ToolFunction, ToolKind, Usage,
};

use super::Client;

impl Client {
    pub(super) fn native_kind(wire: Wire) -> Option<AdapterKind> {
        match wire {
            Wire::Anthropic => Some(AdapterKind::Anthropic),
            Wire::Gemini => Some(AdapterKind::Gemini),
            Wire::Completions => None,
        }
    }

    pub(super) async fn complete_native(
        &self,
        request: &CompletionRequest,
        kind: AdapterKind,
    ) -> Result<AssistantMessage> {
        let genai = self
            .genai
            .as_ref()
            .ok_or_else(|| Error::Config("native wire is missing a genai client".to_string()))?;
        let token = self.auth.token(&self.http).await?;
        let target = ServiceTarget {
            endpoint: GenaiEndpoint::from_owned(self.native_base()),
            auth: AuthData::from_single(token),
            model: ModelIden::new(kind, request.model.clone()),
        };
        let chat = to_chat(request)?;
        let options = chat_options(&self.headers, request.max_output_tokens, request.effort);
        if request.emit_deltas && (self.on_delta.is_some() || self.on_reasoning.is_some()) {
            return self.stream_native(genai, target, chat, &options).await;
        }
        let response = genai
            .exec_chat(target, chat, Some(&options))
            .await
            .map_err(map_genai)?;
        from_response(response)
    }

    async fn stream_native(
        &self,
        genai: &genai::Client,
        target: ServiceTarget,
        chat: ChatRequest,
        options: &ChatOptions,
    ) -> Result<AssistantMessage> {
        let streamed = genai
            .exec_chat_stream(target, chat, Some(options))
            .await
            .map_err(map_genai)?;
        let mut content = String::new();
        let mut reasoning = String::new();
        let mut tool_calls = Vec::new();
        let mut usage = Usage::default();
        let mut stop = StopReason::default();
        let mut stream = streamed.stream;
        while let Some(event) = stream.next().await {
            let event = event.map_err(map_genai)?;
            match event {
                ChatStreamEvent::Chunk(chunk) => {
                    if let Some(emit) = &self.on_delta {
                        emit(&chunk.content);
                    }
                    content.push_str(&chunk.content);
                }
                ChatStreamEvent::ReasoningChunk(chunk) => {
                    if let Some(emit) = &self.on_reasoning {
                        emit(&chunk.content);
                    }
                    reasoning.push_str(&chunk.content);
                }
                ChatStreamEvent::ToolCallChunk(chunk) => {
                    tool_calls.push(from_genai_call(chunk.tool_call));
                }
                ChatStreamEvent::End(end) => {
                    if let Some(captured) = end.captured_content {
                        if content.is_empty()
                            && let Some(text) = captured.first_text()
                        {
                            content = text.to_string();
                        }
                        if tool_calls.is_empty() {
                            tool_calls = captured
                                .into_tool_calls()
                                .into_iter()
                                .map(from_genai_call)
                                .collect();
                        }
                    }
                    if reasoning.is_empty()
                        && let Some(text) = end.captured_reasoning_content
                    {
                        reasoning = text;
                    }
                    if let Some(genai_usage) = end.captured_usage {
                        usage = from_genai_usage(&genai_usage);
                    }
                    if let Some(reason) = end.captured_stop_reason {
                        stop = from_genai_stop(&reason);
                    }
                }
                ChatStreamEvent::Start | ChatStreamEvent::ThoughtSignatureChunk(_) => {}
            }
        }
        Ok(AssistantMessage {
            content: (!content.is_empty()).then_some(content),
            tool_calls,
            reasoning_content: (!reasoning.is_empty()).then_some(reasoning),
            usage,
            stop_reason: stop,
        })
    }

    pub(super) async fn list_native(&self, kind: AdapterKind) -> Result<Vec<String>> {
        let genai = self
            .genai
            .as_ref()
            .ok_or_else(|| Error::Config("native wire is missing a genai client".to_string()))?;
        let token = self.auth.token(&self.http).await?;
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

const THINKING_ROOM: u32 = 32_768;

fn chat_options(
    headers: &[(String, String)],
    max_tokens: u32,
    effort: Option<crate::provider::Effort>,
) -> ChatOptions {
    use crate::provider::Effort;
    use genai::chat::ReasoningEffort;
    let max_tokens = if effort.is_some_and(|effort| effort != Effort::None) {
        max_tokens.max(THINKING_ROOM)
    } else {
        max_tokens
    };
    let mut options = ChatOptions::default()
        .with_max_tokens(max_tokens)
        .with_capture_content(true)
        .with_capture_tool_calls(true)
        .with_capture_usage(true)
        .with_capture_reasoning_content(true)
        .with_normalize_reasoning_content(true);
    if !headers.is_empty() {
        let extra: std::collections::HashMap<String, String> = headers.iter().cloned().collect();
        options = options.with_extra_headers(extra);
    }
    if let Some(effort) = effort.filter(|effort| *effort != Effort::None) {
        options = options.with_reasoning_effort(match effort {
            Effort::None => ReasoningEffort::None,
            Effort::Low => ReasoningEffort::Low,
            Effort::Medium => ReasoningEffort::Medium,
            Effort::High => ReasoningEffort::High,
        });
    }
    options
}

fn to_chat(request: &CompletionRequest) -> Result<ChatRequest> {
    let mut system = String::new();
    let mut messages = Vec::new();
    for message in &request.messages {
        match message.role {
            Role::System => {
                if !system.is_empty() {
                    system.push('\n');
                }
                system.push_str(&message.content);
            }
            Role::User => messages.push(ChatMessage::user(message.content.clone())),
            Role::Assistant => {
                let mut parts = Vec::new();
                if !message.content.is_empty() {
                    parts.push(genai::chat::ContentPart::Text(message.content.clone()));
                }
                for call in &message.tool_calls {
                    parts.push(genai::chat::ContentPart::ToolCall(to_genai_call(call)?));
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
                .map(|tool| Tool {
                    name: ToolName::Custom(tool.function.name.clone()),
                    description: Some(tool.function.description.clone()),
                    schema: Some(tool.function.parameters.clone()),
                    strict: None,
                    config: None,
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
    let fn_arguments =
        serde_json::from_str(&call.function.arguments).unwrap_or_else(|_| serde_json::json!({}));
    Ok(GenaiCall {
        call_id: call.id.clone(),
        fn_name: call.function.name.clone(),
        fn_arguments,
        thought_signatures: None,
    })
}

fn from_genai_call(call: GenaiCall) -> ToolCall {
    ToolCall {
        id: call.call_id,
        kind: ToolKind::Function,
        function: ToolFunction {
            name: call.fn_name,
            arguments: call.fn_arguments.to_string(),
        },
    }
}

fn from_response(response: genai::chat::ChatResponse) -> Result<AssistantMessage> {
    let content = response.first_text().map(str::to_string);
    let reasoning_content = response.reasoning_content.clone();
    let usage = from_genai_usage(&response.usage);
    let stop_reason = response
        .stop_reason
        .as_ref()
        .map(from_genai_stop)
        .unwrap_or_default();
    let tool_calls = response
        .into_tool_calls()
        .into_iter()
        .map(from_genai_call)
        .collect();
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

fn from_genai_stop(reason: &genai::chat::StopReason) -> StopReason {
    match reason {
        genai::chat::StopReason::ToolCall(_) => StopReason::ToolCalls,
        _ => StopReason::Stop,
    }
}

#[cfg(test)]
#[path = "native_tests.rs"]
mod tests;

fn map_genai(error: genai::Error) -> Error {
    let text = error.to_string();
    let lower = text.to_lowercase();
    let kind = if lower.contains("401") || lower.contains("auth") || lower.contains("api key") {
        ProviderFailureKind::Authentication
    } else if lower.contains("429") || lower.contains("rate") {
        ProviderFailureKind::RateLimited
    } else {
        ProviderFailureKind::Http
    };
    Error::provider(kind, text)
}
