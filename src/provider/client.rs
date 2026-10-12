use super::*;
use leyline::http::StatusCode;

mod admit;
mod classify;
mod encode;
mod login;
mod meta;

use encode::*;
#[cfg(test)]
pub(crate) use meta::unreachable_message;
pub(crate) use meta::{input_cap, plan_usage, response_failure};

fn fixed_headers<'a>(raw: impl Iterator<Item = (&'a String, &'a String)>) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = Vec::new();
    for (key, value) in raw {
        let key = key.to_ascii_lowercase();
        headers.retain(|(kept, _)| *kept != key);
        headers.push((key, value.clone()));
    }
    if !headers.iter().any(|(key, _)| key == "user-agent") {
        headers.push((
            "user-agent".to_string(),
            concat!("summonr/", env!("CARGO_PKG_VERSION")).to_string(),
        ));
    }
    headers
}

fn request_timeout(seconds: Option<u64>) -> Result<Option<Duration>> {
    match seconds {
        Some(0) => Err(Error::Config(
            "endpoint idle_timeout_secs must be at least 1".to_string(),
        )),
        Some(seconds) => Ok(Some(crate::limits::seconds(seconds))),
        None => Ok(Some(crate::limits::limits().stream_idle())),
    }
}

#[derive(Clone)]
pub struct Client {
    pub(super) http: leyline::Session,
    pub(super) auth_http: leyline::Session,
    pub(super) base: String,
    pub(super) models_base: String,
    pub(super) auth: Auth,
    pub(super) body: Option<Value>,
    pub(super) headers: Vec<(String, String)>,
    pub(super) opencode: bool,
    pub(super) keep_reasoning: bool,
    pub(super) assistant_after_tool: bool,
    pub(super) images: bool,
    pub(super) name: String,
    pub(super) prompt_cache_key: bool,
    pub(super) cache_ttl: crate::endpoint::CacheTtl,
    pub(super) cache_header: Option<String>,
    pub(super) wire: Wire,
    pub(super) reasoning: Reasoning,
    pub(super) idle: Option<Duration>,
    pub(super) on_delta: Option<DeltaFn>,
    pub(super) on_reasoning: Option<DeltaFn>,
    pub(super) on_tool: Option<DeltaFn>,
}

#[derive(Default)]
pub(super) struct Opts {
    pub wire: Wire,
    pub opencode: bool,
    pub models_base: Option<String>,
    pub prompt_cache_key: bool,
    pub idle_timeout: Option<Duration>,
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
            fixed_headers(std::iter::empty()),
            Opts::default(),
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
        let headers = fixed_headers(endpoint.headers.iter());
        let idle_timeout = request_timeout(endpoint.idle_timeout_secs)?;
        let mut client = Self::build(
            base,
            crate::credentials::auth(endpoint)?,
            endpoint.body.clone(),
            headers,
            Opts {
                wire: endpoint.wire,
                opencode: endpoint.opencode,
                models_base: endpoint.models_url.clone(),
                prompt_cache_key: endpoint.prompt_cache_key,
                idle_timeout,
            },
        )?;
        client.keep_reasoning =
            endpoint.keep_reasoning || wire::traits::traits(&endpoint.model).replay;
        client.images = crate::catalog::accepts_images(endpoint);
        client.assistant_after_tool = endpoint.assistant_after_tool;
        client.name.clone_from(&endpoint.name);
        client.cache_ttl = endpoint.cache_ttl;
        client.cache_header.clone_from(&endpoint.cache_header);
        client.reasoning.clone_from(&endpoint.reasoning);
        Ok(client)
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

    pub(super) async fn token(&self) -> Result<String> {
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

    pub(super) async fn get(&self, url: String) -> Result<Value> {
        let token = self.token().await?;
        let response = self
            .attach(self.http.get(&url), &token, None)
            .send()
            .await
            .map_err(|error| self.unreachable(&error))?;
        if !response.status().is_success() {
            return Err(self.rejected(response, None).await);
        }
        let value: Value = response
            .json()
            .await
            .map_err(|error| Error::provider(ProviderFailureKind::Malformed, error.to_string()))?;
        Ok(value)
    }

    pub(crate) fn request<'a>(
        &self,
        request: &'a CompletionRequest<'a>,
        stream: bool,
    ) -> Result<Body<'a>> {
        let reasoning = &self.reasoning;
        let cache = self.cache_ttl;
        let admitted = self.admit(request);
        let call = &CompletionRequest {
            messages: &admitted,
            ..request.clone()
        };
        let wired = match self.wire {
            Wire::Anthropic => {
                <wire::anthropic::Wire as wire::Chat>::body(call, reasoning, cache, stream)?
            }
            Wire::Gemini => {
                <wire::gemini::Wire as wire::Chat>::body(call, reasoning, cache, stream)?
            }
            Wire::Responses => {
                <wire::responses::Wire as wire::Chat>::body(call, reasoning, cache, stream)?
            }
            Wire::Completions => {
                <wire::completions::Wire as wire::Chat>::body(call, reasoning, cache, stream)?
            }
            Wire::Decisions => {
                return Err(Error::Config(
                    "a classify endpoint does not run sessions".to_string(),
                ));
            }
        };
        let completions = self.wire == Wire::Completions;
        let messages = if completions {
            Some(serde_json::to_vec(&Messages {
                list: call.messages,
                keep_reasoning: self.keeps_reasoning(),
                cached: call.traits.family == wire::traits::Family::Anthropic,
                assistant_after_tool: self.assistant_after_tool,
            })?)
        } else {
            wired.messages
        };
        let tools = request
            .tools_json
            .filter(|_| completions)
            .map(|encoded| encoded.sorted.as_slice());
        let mut body = Body::new(wired.head, wired.slot, messages, tools);
        self.extend_body(&mut body, request);
        Ok(body)
    }

    fn extend_body(&self, body: &mut Body<'_>, request: &CompletionRequest<'_>) {
        if self.prompt_cache_key && !self.auth.is_login() {
            body.set("prompt_cache_key", json!(request.prefix_hash()));
        }
        if self.wire == Wire::Responses
            && self.auth.is_login()
            && let Some(tools) = body.take("tools")
        {
            body.set("tools", json!([wire::responses::namespace(tools)]));
        }
        if let Some(Value::Object(extra)) = &self.body {
            for (key, value) in extra {
                body.set(key, value.clone());
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn request_body(
        &self,
        request: &CompletionRequest<'_>,
        stream: bool,
    ) -> Result<Value> {
        let bytes = self.request(request, stream)?.to_bytes()?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub(super) fn headers(&self, session: Option<&str>) -> Vec<(&str, std::borrow::Cow<'_, str>)> {
        let mut headers: Vec<_> = self
            .headers
            .iter()
            .filter(|(key, _)| {
                let posted = session.is_some();
                !(posted && key == "content-type"
                    || self.opencode
                        && (key == "x-opencode-request" || posted && key == "x-opencode-session"))
            })
            .map(|(key, value)| (key.as_str(), std::borrow::Cow::Borrowed(value.as_str())))
            .collect();
        if self.opencode {
            if let Some(session) = session {
                headers.push((
                    "x-opencode-session",
                    std::borrow::Cow::Owned(session.to_string()),
                ));
            }
            headers.push((
                "x-opencode-request",
                std::borrow::Cow::Owned(uuid::Uuid::now_v7().to_string()),
            ));
        }
        headers
    }

    pub(super) fn url_for<W: wire::Chat>(&self, model: &str, stream: bool) -> String {
        format!("{}{}", self.base, W::path(model, stream))
    }

    fn reach(&self) -> crate::limits::Reach {
        if crate::endpoint::local(&self.base) {
            crate::limits::Reach::Local
        } else {
            crate::limits::Reach::Remote
        }
    }

    pub(super) fn attach(
        &self,
        request: leyline::RequestBuilder,
        token: &str,
        session: Option<&str>,
    ) -> leyline::RequestBuilder {
        let mut request = self
            .auth
            .scheme()
            .apply(request.header("user-agent", summonr_auth::AGENT), token)
            .retry(
                crate::limits::limits()
                    .retry_policy(self.reach())
                    .allow_non_idempotent(true),
            );
        for (key, value) in wire::headers(self.wire) {
            request = request.header(*key, *value);
        }
        for (key, value) in self.auth.stamped() {
            request = request.header(key.as_str(), value.as_str());
        }
        for (key, value) in self.headers(session) {
            request = request.header(key, &*value);
        }
        request
    }

    pub(super) async fn send(
        &self,
        url: &str,
        body: Vec<u8>,
        model: Option<&str>,
        session: &str,
        route: Option<&str>,
        stream: bool,
    ) -> Result<leyline::Response> {
        let response = self.post_json(url, body, session, route, stream).await?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            self.auth.forget_live();
        }
        Err(self.rejected(response, model).await)
    }

    pub(super) async fn post_json(
        &self,
        url: &str,
        body: Vec<u8>,
        session: &str,
        route: Option<&str>,
        stream: bool,
    ) -> Result<leyline::Response> {
        let token = self.token().await?;
        let mut request = self
            .attach(self.http.post(url), &token, Some(session))
            .header("content-type", "application/json");
        if let (Some(name), Some(value)) = (&self.cache_header, route) {
            request = request.header(name.as_str(), value);
        }
        let request = request.body(body);
        let request = if stream { request.stream() } else { request };
        request
            .send()
            .await
            .map_err(|error| self.unreachable(&error))
    }
}

impl Provider for Client {
    fn keeps_reasoning(&self) -> bool {
        self.opencode || self.keep_reasoning
    }

    fn takes_images(&self) -> bool {
        self.images
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
        Box::pin(async move { self.complete_stream(request, partial).await })
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
