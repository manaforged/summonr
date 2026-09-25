use super::*;

mod encode;
mod login;
mod meta;

use encode::*;
pub(crate) use meta::{input_cap, response_failure};

fn request_timeout(seconds: Option<u64>) -> Result<Option<Duration>> {
    match seconds {
        Some(0) => Err(Error::Config(
            "endpoint idle_timeout_secs must be at least 1".to_string(),
        )),
        Some(seconds) => Ok(Some(crate::limits::seconds(seconds))),
        None => Ok(Some(crate::limits::STREAM_IDLE)),
    }
}

#[derive(Clone)]
pub struct Client {
    pub(super) http: leyline::Session,
    pub(super) auth_http: leyline::Session,
    pub(super) base: String,
    pub(super) auth: Auth,
    pub(super) body: Option<Value>,
    pub(super) headers: Vec<(String, String)>,
    pub(super) opencode: bool,
    pub(super) keep_reasoning: bool,
    pub(super) name: String,
    pub(super) prompt_cache_key: bool,
    pub(super) wire: Wire,
    pub(super) on_delta: Option<DeltaFn>,
    pub(super) on_reasoning: Option<DeltaFn>,
    pub(super) on_tool: Option<DeltaFn>,
}

#[derive(Default)]
pub(super) struct Opts {
    pub wire: Wire,
    pub opencode: bool,
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
            Vec::new(),
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
        let headers = endpoint
            .headers
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        let idle_timeout = request_timeout(endpoint.idle_timeout_secs)?;
        let mut client = Self::build(
            base,
            crate::credentials::auth(endpoint)?,
            endpoint.body.clone(),
            headers,
            Opts {
                wire: endpoint.wire,
                opencode: endpoint.opencode,
                prompt_cache_key: endpoint.prompt_cache_key,
                idle_timeout,
            },
        )?;
        client.keep_reasoning = endpoint.keep_reasoning;
        client.name.clone_from(&endpoint.name);
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

    pub(super) fn request_body(
        &self,
        request: &CompletionRequest<'_>,
        stream: bool,
    ) -> Result<Value> {
        let mut body = match self.wire {
            Wire::Anthropic => <wire::anthropic::Wire as wire::Chat>::request(request, stream)?,
            Wire::Gemini => <wire::gemini::Wire as wire::Chat>::request(request, stream)?,
            Wire::Responses => <wire::responses::Wire as wire::Chat>::request(request, stream)?,
            Wire::Completions => {
                let mut body = <wire::completions::Wire as wire::Chat>::request(request, stream)?;
                body["messages"] =
                    serde_json::to_value(wire_messages(request.messages, self.keeps_reasoning())?)?;
                body
            }
            Wire::Decisions => {
                return Err(Error::Config(
                    "a classify endpoint does not run sessions".to_string(),
                ));
            }
        };
        if self.prompt_cache_key {
            body["prompt_cache_key"] = json!(request.prefix_hash());
        }
        if let (Some(Value::Object(extra)), Value::Object(map)) = (&self.body, &mut body) {
            for (key, value) in extra {
                map.insert(key.clone(), value.clone());
            }
        }
        Ok(body)
    }

    pub(super) fn headers(&self, session: Option<&str>) -> HashMap<String, String> {
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

    pub(super) fn url_for<W: wire::Chat>(&self, model: &str, stream: bool) -> String {
        format!("{}{}", self.base, W::path(model, stream))
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
            .retry(crate::limits::retry_policy().allow_non_idempotent(true));
        for (key, value) in wire::headers(self.wire) {
            request = request.header(*key, *value);
        }
        for (key, value) in self.auth.stamped() {
            request = request.header(key.as_str(), value.as_str());
        }
        for (key, value) in self.headers(session) {
            request = request.header(key.as_str(), value.as_str());
        }
        request
    }

    pub(super) async fn send(
        &self,
        url: &str,
        body: &Value,
        session: &str,
        stream: bool,
    ) -> Result<leyline::Response> {
        let response = self.post_json(url, body, session, stream).await?;
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        if status.as_u16() == 401 || status.as_u16() == 403 {
            self.auth.forget_live();
        }
        Err(self
            .rejected(response, body.get("model").and_then(Value::as_str))
            .await)
    }

    pub(super) async fn post_json(
        &self,
        url: &str,
        body: &Value,
        session: &str,
        stream: bool,
    ) -> Result<leyline::Response> {
        let token = self.token().await?;
        let request = self
            .attach(self.http.post(url), &token, Some(session))
            .json(body);
        let request = if stream { request.stream() } else { request };
        request
            .send()
            .await
            .map_err(|error| self.unreachable(&error))
    }

    pub async fn classify(
        &self,
        model: &str,
        state: &Value,
        questions: &std::collections::BTreeMap<String, crate::classify::Question>,
    ) -> Result<crate::classify::Decision> {
        if self.wire != Wire::Decisions {
            return Err(Error::Config(
                "this endpoint does not classify — see `summonr endpoints`".to_string(),
            ));
        }
        let body = wire::decisions::body(model, state, questions);
        let sink = super::capture::Sink::open(self.wire, "classify");
        if let Some(sink) = &sink {
            sink.write("request", &body.to_string());
        }
        let text = self
            .send(&self.base, &body, "classify", false)
            .await?
            .text()
            .await
            .map_err(|error| Error::provider(ProviderFailureKind::Transport, error.to_string()))?;
        if let Some(sink) = &sink {
            sink.write("response", &text);
        }
        wire::decisions::decision(&text)
    }
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
