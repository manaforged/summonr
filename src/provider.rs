use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, ProviderFailure, ProviderFailureKind, Result};

mod auth;
mod native;
mod stream;
mod think;

#[cfg(test)]
use stream::{CallDelta, FunctionDelta, merge_call};
use stream::{Completion, finish};

use crate::endpoint::Wire;
use auth::Auth;

pub(crate) const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

/// Chat role on the OpenAI-compatible wire.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// One image supplied with a user prompt.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Image {
    pub media_type: String,
    #[serde(rename = "data", with = "image_bytes")]
    pub bytes: Vec<u8>,
}

impl Image {
    pub(crate) fn encode(media_type: impl Into<String>, bytes: &[u8]) -> Self {
        Self {
            media_type: media_type.into(),
            bytes: bytes.to_vec(),
        }
    }

    fn url(&self) -> String {
        format!(
            "data:{};base64,{}",
            self.media_type,
            base64::engine::general_purpose::STANDARD.encode(&self.bytes)
        )
    }
}

mod image_bytes {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(bytes))
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(serde::de::Error::custom)
    }
}

/// Text and images supplied together as one user turn.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserInput {
    pub text: String,
    pub images: Vec<Image>,
}

impl From<&str> for UserInput {
    fn from(text: &str) -> Self {
        Self {
            text: text.to_string(),
            images: Vec::new(),
        }
    }
}

impl From<String> for UserInput {
    fn from(text: String) -> Self {
        Self {
            text,
            images: Vec::new(),
        }
    }
}

/// One message in the provider conversation.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Message {
    pub role: Role,
    #[serde(default)]
    pub content: String,
    #[serde(default, skip_serializing)]
    pub images: Vec<Image>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// The model's thinking for this assistant turn. Sent back as
    /// `reasoning_content` so the template can keep it in context.
    #[serde(
        default,
        rename = "reasoning_content",
        skip_serializing_if = "Option::is_none"
    )]
    pub reasoning: Option<String>,
}

impl Message {
    pub(crate) fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            images: Vec::new(),
            tool_calls: Vec::new(),
            tool_call_id: None,
            reasoning: None,
        }
    }

    pub(crate) fn user(input: impl Into<UserInput>) -> Self {
        let input = input.into();
        Self {
            role: Role::User,
            content: input.text,
            images: input.images,
            tool_calls: Vec::new(),
            tool_call_id: None,
            reasoning: None,
        }
    }

    pub(crate) fn tool(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            images: Vec::new(),
            tool_calls: Vec::new(),
            tool_call_id: Some(call_id.into()),
            reasoning: None,
        }
    }
}

/// Tool-call kind on the OpenAI-compatible wire.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolKind {
    #[default]
    Function,
}

/// A function call emitted by an OpenAI-compatible provider.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default)]
    pub kind: ToolKind,
    pub function: ToolFunction,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ToolFunction {
    pub name: String,
    pub arguments: String,
}

/// Token counts reported by a provider.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(from = "UsageWire")]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    /// Completion tokens spent on hidden reasoning, when the provider says.
    #[serde(default)]
    pub reasoning_tokens: u32,
    /// Prompt tokens served from the provider's cache, when it says.
    #[serde(default)]
    pub cached_tokens: u32,
}

/// `usage` as providers send it: the cache count sits under
/// `prompt_tokens_details` (OpenAI, llama.cpp, DeepSeek) or, on some
/// hosts, flat as `cached_tokens` / `prompt_cache_hit_tokens`.
#[derive(Deserialize)]
struct UsageWire {
    #[serde(default)]
    prompt_tokens: u32,
    #[serde(default)]
    completion_tokens: u32,
    #[serde(default)]
    reasoning_tokens: u32,
    #[serde(default)]
    cached_tokens: u32,
    #[serde(default)]
    prompt_cache_hit_tokens: u32,
    #[serde(default)]
    prompt_tokens_details: Option<PromptDetails>,
    #[serde(default)]
    completion_tokens_details: Option<CompletionDetails>,
}

#[derive(Deserialize)]
struct PromptDetails {
    #[serde(default)]
    cached_tokens: u32,
}

#[derive(Deserialize)]
struct CompletionDetails {
    #[serde(default)]
    reasoning_tokens: u32,
}

impl From<UsageWire> for Usage {
    fn from(wire: UsageWire) -> Self {
        let nested = wire
            .prompt_tokens_details
            .map(|details| details.cached_tokens)
            .unwrap_or_default();
        Self {
            prompt_tokens: wire.prompt_tokens,
            completion_tokens: wire.completion_tokens,
            reasoning_tokens: wire
                .completion_tokens_details
                .map(|details| details.reasoning_tokens)
                .unwrap_or_default()
                .max(wire.reasoning_tokens),
            cached_tokens: nested
                .max(wire.cached_tokens)
                .max(wire.prompt_cache_hit_tokens),
        }
    }
}

impl Usage {
    pub fn add(&mut self, other: &Self) {
        self.prompt_tokens = self.prompt_tokens.saturating_add(other.prompt_tokens);
        self.completion_tokens = self
            .completion_tokens
            .saturating_add(other.completion_tokens);
        self.reasoning_tokens = self.reasoning_tokens.saturating_add(other.reasoning_tokens);
        self.cached_tokens = self.cached_tokens.saturating_add(other.cached_tokens);
    }
}

/// The assistant message returned by a provider.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct AssistantMessage {
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
    #[serde(
        default,
        alias = "reasoning",
        deserialize_with = "think::reasoning_field"
    )]
    pub reasoning_content: Option<String>,
    #[serde(default, skip_serializing)]
    pub usage: Usage,
    #[serde(default, skip_serializing)]
    pub stop_reason: StopReason,
}

/// Why the provider ended one model round.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    #[default]
    Stop,
    ToolCalls,
    Length,
    #[serde(other)]
    Other,
}

impl AssistantMessage {
    pub(crate) fn into_message(self) -> Message {
        Message {
            role: Role::Assistant,
            content: self.content.unwrap_or_default(),
            images: Vec::new(),
            tool_calls: self.tool_calls,
            tool_call_id: None,
            reasoning: self.reasoning_content.filter(|text| !text.is_empty()),
        }
    }
}

/// The request supplied to a provider for one model round.
#[derive(Clone, Debug)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub max_output_tokens: u32,
}

impl CompletionRequest {
    /// Stable identity of the request head that should survive appended turns.
    pub(crate) fn prefix_hash(&self) -> String {
        let system = self.messages.first().map(|message| &message.content);
        let bytes = serde_json::to_vec(&(&self.model, system, &self.tools)).unwrap_or_default();
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for byte in bytes {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{hash:016x}")
    }
}

/// A function definition advertised to the provider.
#[derive(Clone, Debug, Serialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub kind: ToolKind,
    pub function: FunctionDefinition,
}

#[derive(Clone, Debug, Serialize)]
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// A provider implementation for one model round.
#[allow(
    async_fn_in_trait,
    reason = "Provider is an application-owned trait with no public Send contract"
)]
pub trait Provider {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>>;
}

type DeltaFn = Arc<dyn Fn(&str) + Send + Sync>;

/// Time allowed for one model round, connect to last byte.
const ROUND_TIMEOUT: Duration = Duration::from_secs(3000);

/// OpenAI-compatible chat client: one `POST {base_url}/chat/completions`
/// per round, streamed when a delta observer is set.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    url: String,
    auth: Auth,
    body: Option<Value>,
    headers: Vec<(String, String)>,
    wire: Wire,
    genai: Option<genai::Client>,
    on_delta: Option<DeltaFn>,
    on_reasoning: Option<DeltaFn>,
}

impl Client {
    /// Host and key only (tests, `MAGE_BASE_URL`).
    pub fn new(api_key: impl Into<String>, base_url: impl Into<String>) -> Result<Self> {
        let mut key = api_key.into();
        if key.trim().is_empty() {
            key = "local".to_string();
        }
        Self::build(
            &base_url.into(),
            Auth::bearer(key),
            None,
            Vec::new(),
            Wire::Completions,
            None,
        )
    }

    /// Connect to a resolved endpoint.
    pub fn connect(endpoint: &crate::endpoint::Endpoint) -> Result<Self> {
        let base = endpoint
            .base_url
            .as_deref()
            .map(str::trim)
            .filter(|base| !base.is_empty())
            .ok_or_else(|| {
                Error::Config(format!("endpoint `{}` needs a base_url", endpoint.name))
            })?;
        let headers = endpoint
            .headers
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let genai = Self::native_kind(endpoint.wire)
            .map(|kind| genai::Client::builder().with_adapter_kind(kind).build());
        Self::build(
            base,
            Auth::from_endpoint(endpoint)?,
            endpoint.body.clone(),
            headers,
            endpoint.wire,
            genai,
        )
    }

    /// RFC 8628 device-code login. Prints the user code and waits for the token.
    pub async fn login(
        endpoint: &crate::endpoint::Endpoint,
        notice: impl FnMut(&str),
    ) -> Result<()> {
        let auth = Auth::from_endpoint(endpoint)?;
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(20))
            .build()
            .map_err(|error| Error::Config(error.to_string()))?;
        auth.login(&http, notice).await
    }

    fn build(
        base: &str,
        auth: Auth,
        body: Option<Value>,
        headers: Vec<(String, String)>,
        wire: Wire,
        genai: Option<genai::Client>,
    ) -> Result<Self> {
        if let Some(extra) = &body {
            let map = extra
                .as_object()
                .ok_or_else(|| Error::Config("endpoint body must be a TOML table".to_string()))?;
            for field in ["model", "messages", "stream", "tools", "stream_options"] {
                if map.contains_key(field) {
                    return Err(Error::Config(format!(
                        "endpoint body cannot override provider-owned `{field}`"
                    )));
                }
            }
        }
        let http = reqwest::Client::builder()
            .timeout(ROUND_TIMEOUT)
            .connect_timeout(Duration::from_secs(20))
            .build()
            .map_err(|error| Error::Config(error.to_string()))?;
        Ok(Self {
            http,
            url: format!("{}/chat/completions", base.trim_end_matches('/')),
            auth,
            body,
            headers,
            wire,
            genai,
            on_delta: None,
            on_reasoning: None,
        })
    }

    /// Print token deltas as they arrive (interactive / print).
    #[must_use]
    pub fn on_delta(mut self, emit: impl Fn(&str) + Send + Sync + 'static) -> Self {
        self.on_delta = Some(Arc::new(emit));
        self
    }

    /// Observe reasoning deltas as they arrive (thinking display).
    #[must_use]
    pub fn on_reasoning(mut self, emit: impl Fn(&str) + Send + Sync + 'static) -> Self {
        self.on_reasoning = Some(Arc::new(emit));
        self
    }

    /// Read the provider credentials from the process environment.
    pub fn from_env() -> Result<Self> {
        let api_key = std::env::var("MAGE_API_KEY").map_err(|error| {
            Error::Config(format!(
                "set MAGE_API_KEY before starting Summoner: {error}"
            ))
        })?;
        let base_url =
            std::env::var("MAGE_BASE_URL").unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
        Self::new(api_key, base_url)
    }

    /// List model identifiers advertised by this OpenAI-compatible host.
    pub async fn models(&self) -> Result<Vec<String>> {
        let base = self
            .url
            .strip_suffix("/chat/completions")
            .unwrap_or(&self.url);
        if let Some(kind) = Self::native_kind(self.wire) {
            return self.list_native(kind).await;
        }
        let token = self.auth.token(&self.http).await?;
        let response = self
            .attach(self.http.get(format!("{base}/models")), &token)
            .send()
            .await
            .map_err(|error| Error::provider(ProviderFailureKind::Transport, error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.unwrap_or_default();
            return Err(response_failure(status.as_u16(), text.trim()));
        }
        let value: Value = response
            .json()
            .await
            .map_err(|error| Error::provider(ProviderFailureKind::Malformed, error.to_string()))?;
        model_ids(&value)
    }

    fn request_body(&self, request: &CompletionRequest, stream: bool) -> Result<Value> {
        let messages = request
            .messages
            .iter()
            .map(message_value)
            .collect::<Result<Vec<_>>>()?;
        let mut body = json!({
            "model": request.model,
            "messages": messages,
            "stream": stream,
            "max_tokens": request.max_output_tokens,
        });
        if !request.tools.is_empty() {
            body["tools"] = serde_json::to_value(&request.tools)?;
        }
        if stream {
            body["stream_options"] = json!({ "include_usage": true });
        }
        if let (Some(Value::Object(extra)), Value::Object(map)) = (&self.body, &mut body) {
            for (key, value) in extra {
                map.insert(key.clone(), value.clone());
            }
        }
        Ok(body)
    }

    fn attach(&self, mut request: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
        request = request.bearer_auth(token);
        for (key, value) in &self.headers {
            request = request.header(key.as_str(), value.as_str());
        }
        request
    }

    async fn send(&self, body: &Value) -> Result<reqwest::Response> {
        let response = self.post_json(body).await?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        if status.as_u16() == 401 || status.as_u16() == 403 {
            self.auth.forget_live();
            let retry = self.post_json(body).await?;
            let code = retry.status().as_u16();
            if retry.status().is_success() {
                return Ok(retry);
            }
            let text = retry.text().await.unwrap_or_default();
            return Err(response_failure(code, text.trim()));
        }
        let text = response.text().await.unwrap_or_default();
        Err(response_failure(status.as_u16(), text.trim()))
    }

    async fn post_json(&self, body: &Value) -> Result<reqwest::Response> {
        let token = self.auth.token(&self.http).await?;
        self.attach(self.http.post(&self.url), &token)
            .json(body)
            .send()
            .await
            .map_err(|error| Error::provider(ProviderFailureKind::Transport, error.to_string()))
    }

    async fn complete_once(&self, request: &CompletionRequest) -> Result<AssistantMessage> {
        let body = self.request_body(request, false)?;
        let reply: Completion =
            self.send(&body).await?.json().await.map_err(|error| {
                Error::provider(ProviderFailureKind::Malformed, error.to_string())
            })?;
        let mut message = reply
            .choices
            .into_iter()
            .next()
            .map(|choice| {
                let mut message = choice.message;
                if let Some(reason) = choice.finish_reason {
                    message.stop_reason = reason;
                }
                message
            })
            .ok_or_else(|| {
                Error::provider(ProviderFailureKind::Malformed, "response has no choices")
            })?;
        message.usage = reply.usage.unwrap_or_default();
        finish(message)
    }
}

fn model_ids(value: &Value) -> Result<Vec<String>> {
    let mut models = value
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .map(str::to_string)
        .collect::<Vec<_>>();
    models.sort_unstable();
    models.dedup();
    if models.is_empty() {
        return Err(Error::provider(
            ProviderFailureKind::Malformed,
            "GET /models returned no model ids",
        ));
    }
    Ok(models)
}

fn response_failure(code: u16, text: &str) -> Error {
    let lower = text.to_lowercase();
    let kind = match code {
        401 | 403 => ProviderFailureKind::Authentication,
        429 => ProviderFailureKind::RateLimited,
        500..=599 => ProviderFailureKind::Server,
        400 if lower.contains("context")
            && ["window", "length", "too long", "exceed"]
                .iter()
                .any(|word| lower.contains(word)) =>
        {
            ProviderFailureKind::ContextOverflow
        }
        _ => ProviderFailureKind::Http,
    };
    Error::Provider(ProviderFailure {
        kind,
        status: Some(code),
        message: format!("HTTP {code}: {text}"),
        partial_output: false,
    })
}

fn message_value(message: &Message) -> Result<Value> {
    let mut value = serde_json::to_value(message)?;
    if message.images.is_empty() {
        return Ok(value);
    }
    let mut parts = Vec::with_capacity(message.images.len() + 1);
    if !message.content.is_empty() {
        parts.push(json!({ "type": "text", "text": message.content }));
    }
    parts.extend(message.images.iter().map(|image| {
        json!({
            "type": "image_url",
            "image_url": { "url": image.url() }
        })
    }));
    value["content"] = Value::Array(parts);
    Ok(value)
}

impl Provider for Client {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(kind) = Self::native_kind(self.wire) {
                return self.complete_native(request, kind).await;
            }
            if self.on_delta.is_some() || self.on_reasoning.is_some() {
                self.complete_stream(request).await
            } else {
                self.complete_once(request).await
            }
        })
    }
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
