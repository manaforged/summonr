use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::rpc::{self, Frame};

pub(super) struct Http {
    client: leyline::Session,
    url: String,
    headers: Vec<(String, String)>,
    session: Option<String>,
    version: Option<String>,
    auth: Option<crate::provider::Auth>,
    next: i64,
}

impl Http {
    pub(super) fn new(
        url: &str,
        headers: &[(String, String)],
        auth: Option<crate::provider::Auth>,
    ) -> Result<Self> {
        Ok(Self {
            client: crate::credentials::http()?,
            url: url.to_string(),
            headers: headers.to_vec(),
            session: None,
            version: None,
            auth,
            next: 1,
        })
    }

    pub(super) async fn initialize(&mut self, params: Value) -> Result<()> {
        let result = self.call("initialize", params).await?;
        self.version = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .map(str::to_string);
        let body = rpc::notification("notifications/initialized", json!({}));
        drop(
            self.post("notifications/initialized", &Value::Null, &body)
                .await?,
        );
        Ok(())
    }

    pub(super) async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = json!(self.next);
        self.next += 1;
        let body = rpc::request(&id, method, params.clone());
        let response = self.post(method, &params, &body).await?;
        if let Some(session) = response.header("mcp-session-id") {
            self.session = Some(session.to_string());
        }
        let status = response.status();
        let stream = response
            .header("content-type")
            .is_some_and(|kind| kind.contains("text/event-stream"));
        let text = response
            .text()
            .await
            .map_err(|error| Error::Tool(format!("`{method}` response broke: {error}")))?;
        if !status.is_success() {
            let head: String = text.chars().take(200).collect();
            return Err(Error::Tool(format!(
                "`{method}` returned HTTP {status}: {head}"
            )));
        }
        let lines: Vec<&str> = if stream {
            text.lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(str::trim)
                .collect()
        } else {
            vec![text.as_str()]
        };
        lines
            .into_iter()
            .find_map(|line| match rpc::parse(line) {
                Ok(Frame::Response {
                    id: got,
                    result,
                    error,
                }) if got == id => Some(rpc::outcome(method, result, error)),
                Ok(Frame::Response { .. } | Frame::Request { .. } | Frame::Notification { .. })
                | Err(_) => None,
            })
            .unwrap_or_else(|| Err(Error::Tool(format!("`{method}` returned no response"))))
    }

    async fn post(&self, method: &str, params: &Value, body: &Value) -> Result<leyline::Response> {
        let mut request = self
            .client
            .post(&self.url)
            .header("user-agent", summonr_auth::AGENT)
            .header("Accept", "application/json, text/event-stream")
            .header("Mcp-Method", method);
        if let Some(name) = params.get("name").and_then(Value::as_str) {
            request = request.header("Mcp-Name", name);
        }
        if let Some(session) = &self.session {
            request = request.header("Mcp-Session-Id", session);
        }
        if let Some(version) = &self.version {
            request = request.header("MCP-Protocol-Version", version);
        }
        for (key, value) in &self.headers {
            request = request.header(key, value);
        }
        if let Some(auth) = &self.auth {
            let token = auth
                .token(&self.client)
                .await
                .map_err(|error| Error::Tool(error.to_string()))?;
            request = request.bearer_auth(&token);
        }
        let request = request.json(body);
        request
            .send()
            .await
            .map_err(|error| Error::Tool(format!("cannot reach the MCP server: {error}")))
    }
}
