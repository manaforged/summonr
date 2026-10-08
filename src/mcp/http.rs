use leyline::http::StatusCode;
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
    inflight: Option<Value>,
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
    reply: Option<Result<Value>>,
    oversize: bool,
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
                crate::limits::limits().connect(),
            ))?,
            url: url.to_string(),
            headers: headers.to_vec(),
            session: None,
            version: None,
            auth,
            expired: false,
            next: 1,
            inflight: None,
        })
    }

    pub(super) fn set_wait(&mut self, wait: Duration) -> Result<()> {
        self.client = crate::credentials::session(summonr_auth::builder(
            wait,
            crate::limits::limits().connect(),
        ))?;
        Ok(())
    }

    pub(super) fn expired(&self) -> bool {
        self.expired
    }

    pub(super) async fn initialize(&mut self, params: Value) -> Result<Option<Value>> {
        let reply = self.exchange("initialize", params, &mut |_| {}).await;
        self.inflight = None;
        let result = match reply {
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
        let reply = self.exchange(method, params, on_note).await;
        self.inflight = None;
        reply.map_err(Failure::error)
    }

    pub(super) async fn cancel_inflight(&mut self) {
        let Some(id) = self.inflight.take() else {
            return;
        };
        let method = "notifications/cancelled";
        let body = rpc::notification(
            method,
            json!({"requestId": id, "reason": "the user stopped the call"}),
        );
        if let Err(error) = self.post(method, &Value::Null, &body).await {
            eprintln!("mcp cancel was not delivered: {error}");
        }
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
        self.inflight = Some(id.clone());
        let response = self
            .post(method, &params, &body)
            .await
            .map_err(Failure::Unreachable)?;
        if let Some(session) = response.header("mcp-session-id") {
            self.session = Some(session.to_string());
        }
        let status = response.status();
        if !status.is_success() {
            let body = response
                .read_until(MCP_FRAME_MAX + 1, |_, _| false)
                .await
                .unwrap_or_default();
            self.expired = status == StatusCode::NOT_FOUND && self.session.is_some();
            let head: String = String::from_utf8_lossy(&body).chars().take(200).collect();
            return Err(Failure::Refused(Error::Tool(format!(
                "`{method}` returned HTTP {status}: {head}"
            ))));
        }
        read_reply(response, &id, method, on_note).await
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

async fn read_reply(
    response: leyline::Response,
    id: &Value,
    method: &str,
    on_note: &mut (dyn FnMut(Notice<'_>) + Send),
) -> std::result::Result<Value, Failure> {
    let stream = response
        .header("content-type")
        .is_some_and(|kind| kind.contains("text/event-stream"));
    let mut events = Events::default();
    let body = response
        .read_until(MCP_FRAME_MAX + 1, |body, _| {
            stream && events.feed(body, id, method, on_note)
        })
        .await
        .map_err(|error| {
            Failure::Unreachable(Error::Tool(format!("`{method}` response broke: {error}")))
        })?;
    if events.reply.is_none() && !events.oversize {
        let rest = body.get(events.start..).unwrap_or_default();
        if stream {
            events.take(rest, id, method, on_note);
        } else {
            events.answer(&String::from_utf8_lossy(rest), id, method, on_note);
        }
    }
    if events.oversize || (events.reply.is_none() && body.len() > MCP_FRAME_MAX) {
        return Err(Failure::Unreachable(Error::Tool(format!(
            "a reply to `{method}` was over {} MiB; summonr dropped it",
            MCP_FRAME_MAX / (1024 * 1024)
        ))));
    }
    events
        .reply
        .unwrap_or_else(|| Err(Error::Tool(format!("`{method}` returned no response"))))
        .map_err(Failure::Refused)
}

impl Events {
    fn feed(
        &mut self,
        body: &[u8],
        id: &Value,
        method: &str,
        on_note: &mut (dyn FnMut(Notice<'_>) + Send),
    ) -> bool {
        while let Some((end, width)) =
            crate::provider::boundary(body, self.searched.max(self.start))
        {
            let event = &body[self.start..end];
            self.start = end + width;
            self.searched = self.start;
            if event.len() > MCP_FRAME_MAX {
                self.oversize = true;
                return true;
            }
            if self.take(event, id, method, on_note) {
                return true;
            }
        }
        if body.len().saturating_sub(self.start) > MCP_FRAME_MAX {
            self.oversize = true;
            return true;
        }
        self.searched = body.len().saturating_sub(3).max(self.start);
        false
    }

    fn take(
        &mut self,
        event: &[u8],
        id: &Value,
        method: &str,
        on_note: &mut (dyn FnMut(Notice<'_>) + Send),
    ) -> bool {
        String::from_utf8_lossy(event)
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .any(|data| self.answer(data, id, method, on_note))
    }

    fn answer(
        &mut self,
        data: &str,
        id: &Value,
        method: &str,
        on_note: &mut (dyn FnMut(Notice<'_>) + Send),
    ) -> bool {
        match rpc::parse(data.trim()) {
            Ok(Frame::Response {
                id: got,
                result,
                error,
            }) if got == *id => {
                self.reply = Some(rpc::outcome(method, result, error));
                true
            }
            Ok(Frame::Notification { method, params }) => {
                on_note(rpc::notice(&method, &params));
                false
            }
            Ok(Frame::Response { .. } | Frame::Request { .. }) | Err(_) => false,
        }
    }
}
