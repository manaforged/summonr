use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, ProviderFailure, ProviderFailureKind, Result};

mod capture;
mod native;
pub(crate) mod stream;
pub(crate) mod think;
mod wire;

use stream::finish;

use crate::endpoint::Wire;
pub(crate) use summonr_auth::Auth;

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
    pub data: String,
}

impl Image {
    pub(crate) fn encode(media_type: impl Into<String>, bytes: &[u8]) -> Self {
        use base64::Engine as _;
        Self {
            media_type: media_type.into(),
            data: base64::engine::general_purpose::STANDARD.encode(bytes),
        }
    }

    pub fn bytes(&self) -> Result<Vec<u8>> {
        base64::engine::general_purpose::STANDARD
            .decode(&self.data)
            .map_err(|error| Error::Config(format!("cannot decode stored image: {error}")))
    }

    fn url(&self) -> String {
        format!("data:{};base64,{}", self.media_type, self.data)
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
    #[serde(
        default,
        deserialize_with = "think::vec_or_null",
        skip_serializing_if = "Vec::is_empty"
    )]
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
    Responses {
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
    #[serde(default, deserialize_with = "think::vec_or_null")]
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
pub struct CompletionRequest<'a> {
    pub session: String,
    pub model: String,
    pub messages: &'a [Message],
    pub tools: &'a [ToolDefinition],
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

    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::None => Self::Low,
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High => Self::None,
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
    pub(crate) fn cost(&self, prompt: u64, completion: u64, cached: u64) -> f64 {
        let cached = cached.min(prompt);
        let fresh = prompt - cached;
        (fresh as f64 * self.input + cached as f64 * self.cached + completion as f64 * self.output)
            / 1_000_000.0
    }
}

#[must_use]
pub(crate) fn compact(count: u64) -> String {
    let value = count as f64;
    if value >= 1e9 {
        format!("{:.2}b", value / 1e9)
    } else if value >= 1e6 {
        format!("{:.1}m", value / 1e6)
    } else if value >= 1e3 {
        format!("{:.1}k", value / 1e3)
    } else {
        count.to_string()
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

impl<'a> CompletionRequest<'a> {
    #[must_use]
    pub fn new(
        session: String,
        model: String,
        messages: &'a [Message],
        tools: &'a [ToolDefinition],
        max_output_tokens: Option<u32>,
        emit_deltas: bool,
        effort: Option<Effort>,
    ) -> Self {
        Self {
            session,
            model,
            messages,
            tools,
            max_output_tokens,
            emit_deltas,
            effort,
        }
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
        request: &'a CompletionRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>>;

    fn stream<'a>(
        &'a self,
        request: &'a CompletionRequest<'a>,
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

    fn keeps_reasoning(&self) -> bool {
        false
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
const STREAM_IDLE: Duration = crate::runtime::JOB_WAIT.saturating_mul(2);
const STREAM_TOTAL: Duration = Duration::from_secs(24 * 60 * 60);

fn request_timeout(seconds: Option<u64>) -> Result<Option<Duration>> {
    match seconds {
        Some(0) => Err(Error::Config(
            "endpoint idle_timeout_secs must be at least 1".to_string(),
        )),
        Some(seconds) => Ok(Some(Duration::from_secs(seconds))),
        None => Ok(Some(STREAM_IDLE)),
    }
}

#[derive(Clone)]
pub struct Client {
    http: leyline::Session,
    auth_http: leyline::Session,
    url: String,
    auth: Auth,
    body: Option<Value>,
    headers: Vec<(String, String)>,
    opencode: bool,
    keep_reasoning: bool,
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
            crate::credentials::bearer(key),
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
        let mut client = Self::build(
            base,
            crate::credentials::auth(endpoint)?,
            endpoint.body.clone(),
            headers,
            endpoint.wire,
            genai,
            idle_timeout,
        )?;
        client.keep_reasoning = endpoint.keep_reasoning;
        Ok(client)
    }

    pub async fn login(
        endpoint: &crate::endpoint::Endpoint,
        prompt: &mut summonr_auth::Prompt,
    ) -> Result<()> {
        let auth = crate::credentials::auth(endpoint)?;
        let http = crate::credentials::http()?;
        auth.login(&http, prompt)
            .await
            .map_err(|error| Error::Config(error.to_string()))
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
        let timeouts = leyline::TimeoutConfig::new()
            .total(STREAM_TOTAL)
            .connect(Duration::from_secs(20))
            .read(idle_timeout);
        let builder = leyline::Session::builder().timeouts(timeouts);
        let mut headers = headers;
        if base.contains("opencode.ai/zen")
            && !headers
                .iter()
                .any(|(key, _)| key.eq_ignore_ascii_case("x-opencode-session"))
        {
            headers.push((
                "x-opencode-session".to_string(),
                uuid::Uuid::now_v7().to_string(),
            ));
        }
        let http = builder
            .build()
            .map_err(|error| Error::Config(error.to_string()))?;
        Ok(Self {
            http,
            auth_http: crate::credentials::http()?,
            url: format!("{}/chat/completions", base.trim_end_matches('/')),
            auth,
            body,
            headers,
            opencode: url::Url::parse(base).is_ok_and(|url| url.host_str() == Some("opencode.ai")),
            keep_reasoning: false,
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

    pub async fn models(&self) -> Result<Vec<String>> {
        Ok(self
            .catalog(crate::catalog::Cost::Unknown)
            .await?
            .into_iter()
            .map(|model| model.id)
            .collect())
    }

    pub async fn catalog(
        &self,
        fallback: crate::catalog::Cost,
    ) -> Result<Vec<crate::catalog::Model>> {
        let base = self
            .url
            .strip_suffix("/chat/completions")
            .unwrap_or(&self.url);
        if let Some(kind) = Self::native_kind(self.wire)
            && self.wire != Wire::Responses
        {
            return Ok(self
                .list_native(kind)
                .await?
                .into_iter()
                .map(|id| crate::catalog::Model {
                    cost: fallback,
                    ..crate::catalog::Model::named(id)
                })
                .collect());
        }
        let value = self.get(format!("{base}/models")).await?;
        let models = crate::catalog::parse(&value, fallback);
        if models.is_empty() {
            return Err(Error::provider(
                ProviderFailureKind::Malformed,
                "GET /models returned no model ids",
            ));
        }
        Ok(models)
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
            Wire::Completions | Wire::Responses | Wire::Decisions => None,
        };
        if let Some(field) = field {
            let mut url = url::Url::parse(&format!("{base}/models/")).ok()?;
            url.path_segments_mut()
                .ok()?
                .pop_if_empty()
                .push(model.strip_prefix("models/").unwrap_or(model));
            let value = self.get(url.to_string()).await.ok()?;
            return value.get(field).and_then(tokens);
        }
        let root = base.strip_suffix("/v1").unwrap_or(base);
        if let Ok(mut url) = url::Url::parse(&format!("{root}/props")) {
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

    async fn token(&self) -> Result<String> {
        self.auth
            .token(&self.auth_http)
            .await
            .map_err(|error| match error {
                summonr_auth::Error::NeedsLogin { .. } | summonr_auth::Error::Denied(_) => {
                    Error::provider(ProviderFailureKind::Authentication, error.to_string())
                }
                summonr_auth::Error::Transport(detail) => {
                    Error::provider(ProviderFailureKind::Transport, detail)
                }
                summonr_auth::Error::Config(detail) => Error::Config(detail),
            })
    }

    async fn get(&self, url: String) -> Result<Value> {
        let token = self.token().await?;
        let mut response = self
            .attach(self.http.get(&url), &token, None)
            .send()
            .await
            .map_err(|error| Error::provider(ProviderFailureKind::Transport, error.to_string()))?;
        let status = response.status();
        if !status.is_success() {
            let after = retry_after(&response);
            let text = response.text().await.unwrap_or_default();
            return Err(response_failure(status.as_u16(), text.trim(), after));
        }
        let value: Value = response
            .json()
            .await
            .map_err(|error| Error::provider(ProviderFailureKind::Malformed, error.to_string()))?;
        Ok(value)
    }

    fn request_body(&self, request: &CompletionRequest<'_>, stream: bool) -> Result<Value> {
        let mut body = <wire::completions::Wire as wire::Chat>::request(request, stream);
        body["messages"] =
            serde_json::to_value(wire_messages(request.messages, self.keeps_reasoning())?)?;
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

    fn headers(&self, session: Option<&str>) -> HashMap<String, String> {
        let mut headers: HashMap<_, _> = self
            .headers
            .iter()
            .map(|(key, value)| (key.to_ascii_lowercase(), value.clone()))
            .collect();
        headers
            .entry("user-agent".to_string())
            .or_insert_with(|| concat!("summonr/", env!("CARGO_PKG_VERSION")).to_string());
        if self.opencode {
            if let Some(session) = session {
                headers.insert("x-opencode-session".to_string(), session.to_string());
            }
            headers.insert(
                "x-opencode-request".to_string(),
                uuid::Uuid::now_v7().to_string(),
            );
        }
        headers
    }

    fn attach(
        &self,
        request: leyline::RequestBuilder,
        token: &str,
        session: Option<&str>,
    ) -> leyline::RequestBuilder {
        let mut request = self
            .auth
            .scheme()
            .apply(request.user_agent(summonr_auth::AGENT), token);
        if self.wire == Wire::Anthropic {
            request = request.header("anthropic-version", "2023-06-01");
        }
        for (key, value) in self.auth.stamped() {
            request = request.header(key.as_str(), value.as_str());
        }
        for (key, value) in self.headers(session) {
            request = request.header(key.as_str(), value.as_str());
        }
        request
    }

    async fn send(&self, body: &Value, session: &str) -> Result<leyline::Response> {
        let mut response = self.post_json(body, session).await?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        if status.as_u16() == 401 || status.as_u16() == 403 {
            self.auth.forget_live();
            let mut retry = self.post_json(body, session).await?;
            let code = retry.status().as_u16();
            if retry.status().is_success() {
                return Ok(retry);
            }
            let after = retry_after(&retry);
            let text = retry.text().await.unwrap_or_default();
            return Err(response_failure(code, text.trim(), after));
        }
        let after = retry_after(&response);
        let text = response.text().await.unwrap_or_default();
        Err(response_failure(status.as_u16(), text.trim(), after))
    }

    async fn post_json(&self, body: &Value, session: &str) -> Result<leyline::Response> {
        let token = self.token().await?;
        self.attach(self.http.post(&self.url), &token, Some(session))
            .json(body)
            .send()
            .await
            .map_err(|error| Error::provider(ProviderFailureKind::Transport, error.to_string()))
    }

    async fn complete_once(&self, request: &CompletionRequest<'_>) -> Result<AssistantMessage> {
        let body = self.request_body(request, false)?;
        let reply: Value = self
            .send(&body, &request.session)
            .await?
            .json()
            .await
            .map_err(|error| Error::provider(ProviderFailureKind::Malformed, error.to_string()))?;
        if reply
            .get("choices")
            .and_then(Value::as_array)
            .is_none_or(|choices| choices.is_empty())
        {
            return Err(Error::provider(
                ProviderFailureKind::Malformed,
                "response has no choices",
            ));
        }
        finish(<wire::completions::Wire as wire::Chat>::parse(
            &request.model,
            &reply,
        ))
    }
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

pub(crate) fn input_cap(text: &str) -> Option<u32> {
    let rest = text.split("maximum allowed length (").nth(1)?;
    let digits: String = rest.chars().take_while(|ch| ch.is_ascii_digit()).collect();
    digits.parse().ok().filter(|value| *value > 0)
}

fn tokens(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
}

pub(crate) fn retry_after(response: &leyline::Response) -> Option<u64> {
    response.header("retry-after")?.trim().parse().ok()
}

pub(crate) fn response_failure(code: u16, text: &str, retry_after: Option<u64>) -> Error {
    let lower = text.to_lowercase();
    let kind = match code {
        401 | 403 => ProviderFailureKind::Authentication,
        429 => ProviderFailureKind::RateLimited,
        400 | 500..=599
            if lower.contains("tool call")
                && lower.contains("arguments")
                && lower.contains("json")
                && (lower.contains("parse")
                    || lower.contains("invalid")
                    || lower.contains("valid")) =>
        {
            ProviderFailureKind::MalformedToolCall
        }
        500..=599 => ProviderFailureKind::Server,
        400 if (lower.contains("context")
            && ["window", "length", "too long", "exceed"]
                .iter()
                .any(|word| lower.contains(word)))
            || lower.contains("maximum allowed length")
            || (lower.contains("input length") && lower.contains("exceed")) =>
        {
            ProviderFailureKind::ContextOverflow
        }
        _ => ProviderFailureKind::Http,
    };
    Error::Provider(ProviderFailure {
        kind,
        status: Some(code),
        message: format!("HTTP {code}: {}", reason(text)),
        partial_output: false,
        retry_after,
    })
}

fn reason(text: &str) -> String {
    let body: Value = serde_json::from_str(text).unwrap_or_default();
    let error = body.get("error").unwrap_or(&body);
    error
        .get("message")
        .or(Some(error))
        .and_then(Value::as_str)
        .map_or_else(|| text.to_string(), str::to_string)
}

fn wire_messages(messages: &[Message], keep_reasoning: bool) -> Result<Vec<Value>> {
    let mut prefix = String::new();
    let mut rest = Vec::new();
    for message in messages {
        if message.role == Role::System && rest.is_empty() {
            if !prefix.is_empty() {
                prefix.push_str("\n\n");
            }
            prefix.push_str(&message.content);
            continue;
        }
        if message.role == Role::System {
            rest.push(message_value(
                &Message::user(message.content.clone()),
                keep_reasoning,
            )?);
        } else {
            rest.push(message_value(message, keep_reasoning)?);
        }
    }
    let mut out = Vec::new();
    if !prefix.is_empty() {
        out.push(message_value(&Message::system(prefix), keep_reasoning)?);
    }
    out.extend(rest);
    Ok(out)
}

fn message_value(message: &Message, keep_reasoning: bool) -> Result<Value> {
    let mut value = serde_json::to_value(message)?;
    if message.role == Role::Assistant
        && let Some(object) = value.as_object_mut()
    {
        if keep_reasoning {
            object
                .entry("reasoning_content")
                .or_insert_with(|| json!(""));
        } else {
            object.remove("reasoning_content");
        }
    }
    if let Some(calls) = value.get_mut("tool_calls").and_then(Value::as_array_mut) {
        for call in calls {
            if let Some(call) = call.as_object_mut() {
                call.remove("continuation");
                if let Some(function) = call.get_mut("function").and_then(Value::as_object_mut)
                    && let Some(arguments) = function.get("arguments").and_then(Value::as_str)
                    && serde_json::from_str::<Value>(arguments).is_err()
                {
                    function.insert("arguments".to_string(), json!("{}"));
                }
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
    fn keeps_reasoning(&self) -> bool {
        self.opencode || self.keep_reasoning
    }

    fn complete<'a>(
        &'a self,
        request: &'a CompletionRequest<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<AssistantMessage>> + Send + 'a>> {
        Box::pin(async move { self.stream(request, &mut Partial::default()).await })
    }

    fn stream<'a>(
        &'a self,
        request: &'a CompletionRequest<'a>,
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
