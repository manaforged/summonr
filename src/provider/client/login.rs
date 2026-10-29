use super::*;

impl Client {
    pub async fn login(
        endpoint: &crate::endpoint::Endpoint,
        prompt: &mut summonr_auth::Prompt,
    ) -> Result<()> {
        if let Transport::ClaudeCode(agent) = Transport::of(endpoint) {
            return agent.login(prompt).await;
        }
        let auth = crate::credentials::auth(endpoint)?;
        let http = crate::credentials::http(crate::limits::limits().auth())?;
        auth.login(&http, prompt)
            .await
            .map_err(|error| Error::Config(error.to_string()))
    }

    pub(super) fn build(
        base: &str,
        auth: Auth,
        body: Option<Value>,
        headers: Vec<(String, String)>,
        opts: Opts,
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
            .total(crate::limits::limits().stream_total())
            .connect(crate::limits::limits().connect())
            .response_header(opts.idle_timeout)
            .read(opts.idle_timeout);
        let builder = leyline::Session::builder().timeout(timeouts);
        let mut headers = headers;
        let named: std::collections::HashSet<String> =
            headers.iter().map(|(key, _)| key.clone()).collect();
        for (key, value) in &opts.session_headers {
            if *value == crate::endpoint::SessionValue::SessionId && !named.contains(key) {
                headers.push((key.clone(), uuid::Uuid::now_v7().to_string()));
            }
        }
        let http = builder
            .build()
            .map_err(|error| Error::Config(error.to_string()))?;
        Ok(Self {
            http,
            auth_http: crate::credentials::http(crate::limits::limits().auth())?,
            base: base.trim_end_matches('/').to_string(),
            models_base: opts
                .models_base
                .unwrap_or_else(|| base.trim_end_matches('/').to_string()),
            auth,
            body,
            headers,
            session_headers: opts.session_headers,
            keep_reasoning: false,
            assistant_after_tool: false,
            images: true,
            name: String::new(),
            prompt_cache_key: opts.prompt_cache_key,
            cache_ttl: crate::limits::limits().cache_ttl,
            cache_marks: false,
            transport: wire::codec(opts.wire).map_or(Transport::Classify, |_| Transport::Http),
            wire: opts.wire,
            reasoning: Reasoning::default(),
            idle: opts.idle_timeout,
            on_stream: None,
        })
    }
}
