use super::*;

impl Client {
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
            .read(opts.idle_timeout);
        let builder = leyline::Session::builder().timeout(timeouts);
        let mut headers = headers;
        if opts.opencode
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
            base: base.trim_end_matches('/').to_string(),
            models_base: opts
                .models_base
                .unwrap_or_else(|| base.trim_end_matches('/').to_string()),
            auth,
            body,
            headers,
            opencode: opts.opencode,
            keep_reasoning: false,
            name: String::new(),
            prompt_cache_key: opts.prompt_cache_key,
            cache_ttl: crate::limits::limits().cache_ttl,
            cache_header: None,
            wire: opts.wire,
            reasoning: Reasoning::default(),
            idle: opts.idle_timeout,
            on_delta: None,
            on_reasoning: None,
            on_tool: None,
        })
    }
}
