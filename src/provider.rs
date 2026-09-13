use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, ProviderFailure, ProviderFailureKind, Result};

mod auth;
pub(crate) use auth::forget_legacy;
mod native;
mod stream;
mod think;

#[cfg(test)]
use stream::{CallDelta, FunctionDelta, merge_call};
use stream::{Completion, finish};

use crate::endpoint::Wire;
use auth::Auth;

pub(crate) const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UserInput {
    pub text: String,
    pub images: Vec<Image>,
    pub source: Option<Source>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub text: String,
    pub images: usize,
}

impl From<&str> for UserInput {
    fn from(text: &str) -> Self {
        Self {
            text: text.to_string(),
            images: Vec::new(),
            source: None,
        }
    }
}

impl From<String> for UserInput {
    fn from(text: String) -> Self {
        Self {
            text,
            images: Vec::new(),
            source: None,
        }
    }
}

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

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ToolKind {
    #[default]
    Function,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default)]
    pub kind: ToolKind,
    pub function: ToolFunction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<Continuation>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(tag = "provider", rename_all = "snake_case")]
pub enum Continuation {
    Anthropic {
        model: String,
        blocks: Vec<Value>,
    },
    Gemini {
        model: String,
        signatures: Vec<String>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ToolFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(from = "UsageWire")]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u32,
    #[serde(default)]
    pub completion_tokens: u32,
    #[serde(default)]
    pub reasoning_tokens: u32,
    #[serde(default)]
    pub cached_tokens: u32,
}

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

#[derive(Clone, Debug)]
pub struct CompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolDefinition>,
    pub max_output_tokens: Option<u32>,
    pub emit_deltas: bool,
    pub effort: Option<Effort>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    None,
    Low,
    Medium,
    High,
}

impl<'de> Deserialize<'de> for Effort {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| {
            serde::de::Error::custom(format!(
                "effort takes none, low, medium, or high, not `{text}`"
            ))
        })
    }
}

impl Effort {
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "none" | "off" => Some(Self::None),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub cached: f64,
}

impl Price {
    #[must_use]
    pub fn cost(&self, prompt: u64, completion: u64, cached: u64) -> f64 {
        let cached = cached.min(prompt);
        let fresh = prompt - cached;
        (fresh as f64 * self.input + cached as f64 * self.cached + completion as f64 * self.output)
            / 1_000_000.0
    }
}

#[must_use]
pub fn dollars(amount: f64) -> String {
    if amount < 0.01 {
        format!("${amount:.4}")
    } else {
        format!("${amount:.2}")
    }
}

impl CompletionRequest {
    pub(crate) fn durable_bytes(&self) -> serde_json::Result<Vec<u8>> {
        #[derive(Serialize)]
        struct DurableMessage<'a> {
            role: Role,
            content: &'a str,
            images: &'a [Image],
            tool_calls: &'a [ToolCall],
            tool_call_id: Option<&'a str>,
            reasoning: Option<&'a str>,
        }

        #[derive(Serialize)]
        struct DurableRequest<'a> {
            model: &'a str,
            messages: Vec<DurableMessage<'a>>,
            tools: &'a [ToolDefinition],
            max_output_tokens: Option<u32>,
        }

        let messages = self
            .messages
            .iter()
            .map(|message| DurableMessage {
                role: message.role,
                content: &message.content,
                images: &message.images,
                tool_calls: &message.tool_calls,
                tool_call_id: message.tool_call_id.as_deref(),
                reasoning: message.reasoning.as_deref(),
            })
            .collect();
        serde_json::to_vec(&DurableRequest {
            model: &self.model,
            messages,
            tools: &self.tools,
            max_output_tokens: self.max_output_tokens,
        })
    }

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

#[allow(
    async_fn_in_trait,
    reason = "Provider is an application-owned trait with no public Send contract"
)]
pub trait Provider: Send + Sync + 'static {
    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>>;

    fn stream<'a>(
        &'a self,
        request: &'a CompletionRequest,
        _partial: &'a mut Partial,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        self.complete(request)
    }

    fn child(&self) -> Option<Self>
    where
        Self: Sized,
    {
        None
    }

    fn supports_children(&self) -> bool {
        false
    }
}

type DeltaFn = Arc<dyn Fn(&str) + Send + Sync>;

#[derive(Default)]
pub struct Partial {
    pub text: String,
    pub reasoning: String,
}

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

fn request_timeout(seconds: Option<u64>) -> Result<Option<Duration>> {
    match seconds {
        Some(0) => Err(Error::Config(
            "endpoint idle_timeout_secs must be at least 1".to_string(),
        )),
        Some(seconds) => Ok(Some(Duration::from_secs(seconds))),
        None => Ok(None),
    }
}

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
    on_tool: Option<DeltaFn>,
}

impl Client {
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
            None,
        )
    }

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
        let idle_timeout = request_timeout(endpoint.idle_timeout_secs)?;
        let genai = Self::native_kind(endpoint.wire)
            .map(|kind| {
                let builder = genai::Client::builder().with_adapter_kind(kind);
                let builder = if let Some(timeout) = idle_timeout {
                    builder.with_web_config(genai::WebConfig {
                        read_timeout: Some(timeout),
                        ..genai::WebConfig::default()
                    })
                } else {
                    builder
                };
                builder.build()
            })
            .transpose()
            .map_err(native::map_genai)?;
        Self::build(
            base,
            Auth::from_endpoint(endpoint)?,
            endpoint.body.clone(),
            headers,
            endpoint.wire,
            genai,
            idle_timeout,
        )
    }

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
        idle_timeout: Option<Duration>,
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
        let mut builder = reqwest::Client::builder().connect_timeout(Duration::from_secs(20));
        if let Some(timeout) = idle_timeout {
            builder = builder.read_timeout(timeout);
        }
        let http = builder
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
            on_tool: None,
        })
    }

    #[must_use]
    pub fn on_delta(mut self, emit: impl Fn(&str) + Send + Sync + 'static) -> Self {
        self.on_delta = Some(Arc::new(emit));
        self
    }

    #[must_use]
    pub fn on_reasoning(mut self, emit: impl Fn(&str) + Send + Sync + 'static) -> Self {
        self.on_reasoning = Some(Arc::new(emit));
        self
    }

    #[must_use]
    pub fn on_tool(mut self, emit: impl Fn(&str) + Send + Sync + 'static) -> Self {
        self.on_tool = Some(Arc::new(emit));
        self
    }

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

    pub async fn models(&self) -> Result<Vec<String>> {
        let base = self
            .url
            .strip_suffix("/chat/completions")
            .unwrap_or(&self.url);
        if let Some(kind) = Self::native_kind(self.wire) {
            return self.list_native(kind).await;
        }
        let value = self.get(format!("{base}/models")).await?;
        model_ids(&value)
    }

    pub async fn context(&self, model: &str) -> Option<u32> {
        tokio::time::timeout(PROBE_TIMEOUT, self.probe(model))
            .await
            .ok()
            .flatten()
    }

    async fn probe(&self, model: &str) -> Option<u32> {
        let base = self
            .url
            .strip_suffix("/chat/completions")
            .unwrap_or(&self.url);
        let field = match self.wire {
            Wire::Anthropic => Some("max_input_tokens"),
            Wire::Gemini => Some("inputTokenLimit"),
            Wire::Completions => None,
        };
        if let Some(field) = field {
            let mut url = reqwest::Url::parse(&format!("{base}/models/")).ok()?;
            url.path_segments_mut()
                .ok()?
                .pop_if_empty()
                .push(model.strip_prefix("models/").unwrap_or(model));
            let value = self.get(url.to_string()).await.ok()?;
            return value.get(field).and_then(tokens);
        }
        let root = base.strip_suffix("/v1").unwrap_or(base);
        if let Ok(mut url) = reqwest::Url::parse(&format!("{root}/props")) {
            url.query_pairs_mut().append_pair("model", model);
            if let Ok(value) = self.get(url.to_string()).await
                && let Some(limit) = loaded(&value)
            {
                return Some(limit);
            }
        }
        let value = self.get(format!("{base}/models")).await.ok()?;
        listed(&value, model)
    }

    async fn get(&self, url: String) -> Result<Value> {
        let token = self.auth.token(&self.http).await?;
        let response = self
            .attach(self.http.get(url), &token)
            .send()
            .await
            .map_err(|error| Error::provider(ProviderFailureKind::Transport, error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            let after = retry_after(response.headers());
            let text = response.text().await.unwrap_or_default();
            return Err(response_failure(status.as_u16(), text.trim(), after));
        }
        let value: Value = response
            .json()
            .await
            .map_err(|error| Error::provider(ProviderFailureKind::Malformed, error.to_string()))?;
        Ok(value)
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
        });
        if let Some(max) = request.max_output_tokens {
            body["max_tokens"] = json!(max);
        }
        if !request.tools.is_empty() {
            body["tools"] = serde_json::to_value(&request.tools)?;
        }
        if stream {
            body["stream_options"] = json!({ "include_usage": true });
        }
        if let Some(effort) = request.effort {
            body["reasoning_effort"] = json!(effort.as_str());
        }
        if self.url.contains("api.openai.com") {
            body["prompt_cache_key"] = json!(request.prefix_hash());
        }
        if let (Some(Value::Object(extra)), Value::Object(map)) = (&self.body, &mut body) {
            for (key, value) in extra {
                map.insert(key.clone(), value.clone());
            }
        }
        Ok(body)
    }

    fn attach(&self, mut request: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
        request = match self.wire {
            Wire::Completions => request.bearer_auth(token),
            Wire::Anthropic => request
                .header("x-api-key", token)
                .header("anthropic-version", "2023-06-01"),
            Wire::Gemini => request.header("x-goog-api-key", token),
        };
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
            let after = retry_after(retry.headers());
            let text = retry.text().await.unwrap_or_default();
            return Err(response_failure(code, text.trim(), after));
        }
        let after = retry_after(response.headers());
        let text = response.text().await.unwrap_or_default();
        Err(response_failure(status.as_u16(), text.trim(), after))
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

fn loaded(value: &Value) -> Option<u32> {
    value
        .pointer("/default_generation_settings/n_ctx")
        .or_else(|| value.get("n_ctx"))
        .and_then(tokens)
}

fn listed(value: &Value, model: &str) -> Option<u32> {
    let rows = value.get("data")?.as_array()?;
    let row = rows
        .iter()
        .find(|row| row.get("id").and_then(Value::as_str) == Some(model))?;
    [
        row.get("context_length"),
        row.get("context_window"),
        row.get("max_model_len"),
        row.get("max_context_length"),
        row.pointer("/top_provider/context_length"),
    ]
    .into_iter()
    .flatten()
    .find_map(tokens)
}

fn tokens(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
}

fn retry_after(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}

fn response_failure(code: u16, text: &str, retry_after: Option<u64>) -> Error {
    let lower = text.to_lowercase();
    let kind = match code {
        401 | 403 => ProviderFailureKind::Authentication,
        429 => ProviderFailureKind::RateLimited,
        500..=599
            if lower.contains("tool call")
                && lower.contains("arguments")
                && lower.contains("json")
                && (lower.contains("parse") || lower.contains("invalid")) =>
        {
            ProviderFailureKind::MalformedToolCall
        }
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
        retry_after,
    })
}

fn message_value(message: &Message) -> Result<Value> {
    let mut value = serde_json::to_value(message)?;
    if let Some(calls) = value.get_mut("tool_calls").and_then(Value::as_array_mut) {
        for call in calls {
            if let Some(call) = call.as_object_mut() {
                call.remove("continuation");
            }
        }
    }
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
        Box::pin(async move { self.stream(request, &mut Partial::default()).await })
    }

    fn stream<'a>(
        &'a self,
        request: &'a CompletionRequest,
        partial: &'a mut Partial,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(kind) = Self::native_kind(self.wire) {
                return self.complete_native(request, kind, partial).await;
            }
            if request.emit_deltas && (self.on_delta.is_some() || self.on_reasoning.is_some()) {
                self.complete_stream(request, partial).await
            } else {
                self.complete_once(request).await
            }
        })
    }

    fn child(&self) -> Option<Self> {
        let mut child = self.clone();
        child.on_delta = None;
        child.on_reasoning = None;
        child.on_tool = None;
        Some(child)
    }

    fn supports_children(&self) -> bool {
        true
    }
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod tests;
