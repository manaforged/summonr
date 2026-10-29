use std::collections::VecDeque;
use std::time::Duration;

use futures::StreamExt;
use leyline::http::StatusCode;
use serde_json::{Value, json};

use crate::rpc::{HttpTransport, Pending, Read};
use summonr_core::error::{Error, Result};

pub(crate) struct Http {
    client: leyline::Session,
    url: String,
    headers: Vec<(String, String)>,
    session: Option<String>,
    version: Option<String>,
    auth: Option<crate::provider::Auth>,
    expired: bool,
    body: Option<Body>,
}

struct Body {
    stream: leyline::BodyStream,
    buffer: Vec<u8>,
    searched: usize,
    sse: bool,
    done: bool,
    queue: VecDeque<Read>,
}

impl Http {
    pub(crate) fn new(
        url: &str,
        headers: &[(String, String)],
        wait: Duration,
        auth: Option<crate::provider::Auth>,
    ) -> Result<Self> {
        let limits = crate::limits::limits();
        let client = crate::credentials::session(
            summonr_auth::builder(
                wait.saturating_mul(limits.mcp_progress_stretch),
                limits.connect(),
            ),
            limits.retry_policy(crate::limits::Reach::Remote),
        )?;
        Ok(Self {
            client,
            url: url.to_string(),
            headers: headers.to_vec(),
            session: None,
            version: None,
            auth,
            expired: false,
            body: None,
        })
    }

    async fn deliver(&mut self, value: &Value) -> Result<Option<Value>> {
        let method = value.get("method").and_then(Value::as_str);
        let asks = method.is_some() && value.get("id").is_some_and(|id| !id.is_null());
        let response = self.post(method, value).await?;
        if let Some(session) = response.header("mcp-session-id") {
            self.session = Some(session.to_string());
        }
        let status = response.status();
        if !status.is_success() {
            let body = response
                .read_until(crate::limits::limits().mcp_frame_max_bytes + 1, |_, _| {
                    false
                })
                .await
                .unwrap_or_default();
            self.expired = status == StatusCode::NOT_FOUND && self.session.is_some();
            let head: String = String::from_utf8_lossy(&body).chars().take(200).collect();
            return Ok(Some(json!({"message": format!("HTTP {status}: {head}")})));
        }
        if asks {
            self.body = Some(Body::new(response)?);
        }
        Ok(None)
    }

    async fn read(&mut self) -> std::io::Result<Read> {
        let Some(body) = self.body.as_mut() else {
            return Ok(Read::Closed);
        };
        let read = body.next(crate::limits::limits().mcp_frame_max_bytes).await;
        if !matches!(read, Ok(Read::Line(_))) {
            self.body = None;
        }
        read
    }

    async fn post(&self, method: Option<&str>, body: &Value) -> Result<leyline::Response> {
        let mut request = self
            .client
            .post(&self.url)
            .header("user-agent", summonr_auth::AGENT)
            .header("Accept", "application/json, text/event-stream");
        if let Some(method) = method {
            request = request.header("Mcp-Method", method);
        }
        if let Some(name) = body.pointer("/params/name").and_then(Value::as_str) {
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
        request
            .json(body)
            .stream()
            .send()
            .await
            .map_err(|error| Error::Tool(format!("cannot reach the MCP server: {error}")))
    }
}

impl HttpTransport for Http {
    fn send<'a>(&'a mut self, value: &'a Value) -> Pending<'a, Result<Option<Value>>> {
        Box::pin(self.deliver(value))
    }

    fn next(&mut self) -> Pending<'_, std::io::Result<Read>> {
        Box::pin(self.read())
    }

    fn expired(&self) -> bool {
        self.expired
    }

    fn agree(&mut self, version: &str) {
        self.version = Some(version.to_string());
    }
}

impl Body {
    fn new(response: leyline::Response) -> Result<Self> {
        let sse = response
            .header("content-type")
            .is_some_and(|kind| kind.contains("text/event-stream"));
        let stream = response
            .into_decoded_stream(None)
            .map_err(|error| Error::Tool(format!("MCP response broke: {error}")))?;
        Ok(Self {
            stream,
            buffer: Vec::new(),
            searched: 0,
            sse,
            done: false,
            queue: VecDeque::new(),
        })
    }

    async fn next(&mut self, max: usize) -> std::io::Result<Read> {
        loop {
            if let Some(read) = self.queue.pop_front() {
                return Ok(read);
            }
            if self.done {
                return Ok(Read::Closed);
            }
            match self.stream.next().await {
                Some(chunk) => self.buffer.extend_from_slice(&chunk?),
                None => {
                    self.done = true;
                    let rest = std::mem::take(&mut self.buffer);
                    self.push(&rest, max);
                    continue;
                }
            }
            if self.sse {
                while let Some((end, width)) =
                    crate::provider::boundary(&self.buffer, self.searched)
                {
                    let event: Vec<u8> = self.buffer.drain(..end + width).collect();
                    self.push(&event[..end], max);
                    self.searched = 0;
                }
                self.searched = self.buffer.len().saturating_sub(3);
            }
            if self.buffer.len() > max {
                self.done = true;
                self.buffer = Vec::new();
                self.queue.push_back(Read::Oversize);
            }
        }
    }

    fn push(&mut self, event: &[u8], max: usize) {
        if event.len() > max {
            self.queue.push_back(Read::Oversize);
            return;
        }
        let text = String::from_utf8_lossy(event);
        let data = if self.sse {
            text.lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(str::trim_start)
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            text.into_owned()
        };
        if !data.trim().is_empty() {
            self.queue.push_back(Read::Line(data));
        }
    }
}
