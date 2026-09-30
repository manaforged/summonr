use serde_json::{Value, json};

use std::time::Duration;

use crate::error::{Error, Result};
use crate::limits::MCP_FRAME_MAX;
use crate::rpc::{self, Frame, Notice};

pub(super) struct Http {
    client: leyline::Session,
    url: String,
    headers: Vec<(String, String)>,
    session: Option<String>,
    version: Option<String>,
    auth: Option<crate::provider::Auth>,
    expired: bool,
    next: i64,
}

enum Failure {
    Unreachable(Error),
    Refused(Error),
}

impl Failure {
    fn error(self) -> Error {
        match self {
            Self::Unreachable(error) | Self::Refused(error) => error,
        }
    }
}

#[derive(Default)]
struct Events {
    start: usize,
    searched: usize,
}

impl Http {
    pub(super) fn new(
        url: &str,
        headers: &[(String, String)],
        wait: Duration,
        auth: Option<crate::provider::Auth>,
    ) -> Result<Self> {
        Ok(Self {
            client: crate::credentials::session(summonr_auth::builder(
                wait,
                crate::limits::CONNECT,
            ))?,
            url: url.to_string(),
            headers: headers.to_vec(),
            session: None,
            version: None,
            auth,
            expired: false,
            next: 1,
        })
    }

    pub(super) fn set_wait(&mut self, wait: Duration) -> Result<()> {
        self.client =
            crate::credentials::session(summonr_auth::builder(wait, crate::limits::CONNECT))?;
        Ok(())
    }

    pub(super) fn expired(&self) -> bool {
        self.expired
    }

    pub(super) async fn initialize(&mut self, params: Value) -> Result<Option<Value>> {
        let result = match self.exchange("initialize", params, &mut |_| {}).await {
            Ok(result) => result,
            Err(Failure::Refused(_)) => return Ok(None),
            Err(Failure::Unreachable(error)) => return Err(error),
        };
        rpc::agreed(&result)?;
        self.version = result
            .get("protocolVersion")
            .and_then(Value::as_str)
            .map(str::to_string);
        let body = rpc::notification("notifications/initialized", json!({}));
        drop(
            self.post("notifications/initialized", &Value::Null, &body)
                .await?,
        );
        Ok(Some(result))
    }

    pub(super) async fn call(
        &mut self,
        method: &str,
        params: Value,
        on_note: &mut (dyn FnMut(Notice<'_>) + Send),
    ) -> Result<Value> {
        self.exchange(method, params, on_note)
            .await
            .map_err(Failure::error)
    }

    async fn exchange(
        &mut self,
        method: &str,
        params: Value,
        on_note: &mut (dyn FnMut(Notice<'_>) + Send),
    ) -> std::result::Result<Value, Failure> {
        let id = json!(self.next);
        self.next += 1;
        let body = rpc::request(&id, method, params.clone());
        let response = self
            .post(method, &params, &body)
            .await
            .map_err(Failure::Unreachable)?;
        if let Some(session) = response.header("mcp-session-id") {
            self.session = Some(session.to_string());
        }
        let status = response.status();
        let stream = response
            .header("content-type")
            .is_some_and(|kind| kind.contains("text/event-stream"));
        let mut events = Events::default();
        let body = response
            .read_until(MCP_FRAME_MAX + 1, |body, _| {
                stream && events.replied(body, &id)
            })
            .await
            .map_err(|error| {
                Failure::Unreachable(Error::Tool(format!("`{method}` response broke: {error}")))
            })?;
        if body.len() > MCP_FRAME_MAX {
            return Err(Failure::Unreachable(Error::Tool(format!(
                "a reply to `{method}` was over {} MiB; summonr dropped it",
                MCP_FRAME_MAX / (1024 * 1024)
            ))));
        }
        let text = String::from_utf8_lossy(&body);
        if !status.is_success() {
            self.expired = status.as_u16() == 404 && self.session.is_some();
            let head: String = text.chars().take(200).collect();
            return Err(Failure::Refused(Error::Tool(format!(
                "`{method}` returned HTTP {status}: {head}"
            ))));
        }
        let lines: Vec<&str> = if stream {
            text.lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(str::trim)
                .collect()
        } else {
            vec![text.as_ref()]
        };
        lines
            .into_iter()
            .find_map(|line| match rpc::parse(line) {
                Ok(Frame::Response {
                    id: got,
                    result,
                    error,
                }) if got == id => Some(rpc::outcome(method, result, error)),
                Ok(Frame::Notification { method, params }) => {
                    on_note(rpc::notice(&method, &params));
                    None
                }
                Ok(Frame::Response { .. } | Frame::Request { .. }) | Err(_) => None,
            })
            .unwrap_or_else(|| Err(Error::Tool(format!("`{method}` returned no response"))))
            .map_err(Failure::Refused)
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
        let request = request.json(body).stream();
        request
            .send()
            .await
            .map_err(|error| Error::Tool(format!("cannot reach the MCP server: {error}")))
    }
}

impl Events {
    fn replied(&mut self, body: &[u8], id: &Value) -> bool {
        while let Some((end, width)) =
            crate::provider::boundary(body, self.searched.max(self.start))
        {
            let event = &body[self.start..end];
            self.start = end + width;
            self.searched = self.start;
            if answers(event, id) {
                return true;
            }
        }
        self.searched = body.len().saturating_sub(3).max(self.start);
        false
    }
}

fn answers(event: &[u8], id: &Value) -> bool {
    String::from_utf8_lossy(event)
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .any(|data| {
            matches!(rpc::parse(data.trim()), Ok(Frame::Response { id: got, .. }) if got == *id)
        })
}
