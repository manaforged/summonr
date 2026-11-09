use std::path::Path;
use std::time::Duration;

use tokio::time::Instant;

use serde_json::{Value, json};

use super::frame::{Framing, Lines, Read};
use super::{Peer, notification, request};
use crate::process::{Cap, Child, Env, Spec, Tail};
use summonr_core::error::{Error, Result};

mod reply;
mod transport;

pub(crate) use transport::{HttpTransport, Pending};
use transport::{Sent, Stdio, Transport};

pub(crate) struct Client {
    transport: Transport,
    max: usize,
    peer: Peer,
    abandoned: Vec<Value>,
    inflight: Vec<Value>,
    roots: Value,
    broken: bool,
    wait: Duration,
    next: i64,
}

pub(crate) struct Spawn<'a> {
    pub(crate) program: &'a str,
    pub(crate) args: &'a [&'a str],
    pub(crate) env: &'a [(String, String)],
    pub(crate) cwd: &'a Path,
    pub(crate) wait: Duration,
    pub(crate) base: Env,
    pub(crate) framing: Framing,
    pub(crate) max: usize,
    pub(crate) peer: Peer,
}

impl Client {
    pub(crate) fn spawn(launch: &Spawn<'_>) -> Result<Self> {
        let process = crate::process::start(
            Spec {
                cwd: Some(launch.cwd.to_path_buf()),
                base: launch.base,
                ..Spec::new(launch.program, launch.args)
            }
            .envs(launch.env.iter().cloned()),
        )?;
        let stderr = Tail::spawn(
            process.stderr,
            Cap::Tail(crate::limits::limits().mcp_stderr_tail_bytes),
        );
        let stdio = Stdio {
            group: process.group,
            child: Some(process.child),
            stdin: Some(process.stdin),
            reader: Lines::framed(process.stdout, launch.framing, launch.max),
            framing: launch.framing,
            stderr,
            secrets: launch.env.iter().map(|(_, value)| value.clone()).collect(),
        };
        Ok(Self::over(
            Transport::Stdio(Box::new(stdio)),
            launch.cwd,
            launch.wait,
            launch.max,
            launch.peer.clone(),
        ))
    }

    pub(crate) fn attach(http: Box<dyn HttpTransport>, cwd: &Path, wait: Duration) -> Self {
        Self::over(
            Transport::Http(http),
            cwd,
            wait,
            crate::limits::limits().mcp_frame_max_bytes,
            Peer::Mcp,
        )
    }

    fn over(transport: Transport, cwd: &Path, wait: Duration, max: usize, peer: Peer) -> Self {
        Self {
            transport,
            max,
            peer,
            abandoned: Vec::new(),
            inflight: Vec::new(),
            roots: roots(cwd),
            broken: false,
            wait,
            next: 1,
        }
    }

    pub(crate) fn pid(&self) -> Option<u32> {
        match &self.transport {
            Transport::Stdio(stdio) => stdio.child.as_ref().and_then(Child::id),
            Transport::Http(_) => None,
        }
    }

    pub(crate) fn piped(&self) -> bool {
        self.transport.piped()
    }

    pub(crate) fn set_wait(&mut self, wait: Duration) {
        self.wait = wait;
    }

    pub(crate) fn agree(&mut self, version: &str) {
        if let Transport::Http(http) = &mut self.transport {
            http.agree(version);
        }
    }

    pub(crate) async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.write(&notification(method, params)).await
    }

    pub(crate) async fn cancel_inflight(&mut self) {
        for id in std::mem::take(&mut self.inflight) {
            self.cancel(&id, "the user stopped the call").await;
            self.abandoned.push(id);
        }
    }

    async fn cancel(&mut self, id: &Value, reason: &str) {
        if !self.peer.cancels() {
            return;
        }
        let cancel = notification(
            "notifications/cancelled",
            json!({"requestId": id, "reason": reason}),
        );
        let deadline = Instant::now() + crate::limits::limits().mcp_cancel_write();
        let sent = self.transport.send(&cancel, deadline).await;
        if self.transport.piped() && !matches!(sent, Ok(Sent::Delivered)) {
            self.broken = true;
        }
    }

    pub(crate) async fn call(
        &mut self,
        method: &str,
        params: Value,
        on_note: impl FnMut(super::Notice<'_>),
    ) -> Result<Value> {
        self.attempt(method, params, on_note).await?
    }

    pub(crate) async fn attempt(
        &mut self,
        method: &str,
        params: Value,
        mut on_note: impl FnMut(super::Notice<'_>),
    ) -> Result<Result<Value>> {
        let (result, error) = self.exchange(method, params.clone(), &mut on_note).await?;
        let (result, error) = if self.peer.retries(error.as_ref()) {
            self.exchange(method, params, &mut on_note).await?
        } else {
            (result, error)
        };
        Ok(super::outcome(self.peer.label(method), result, error))
    }

    async fn exchange(
        &mut self,
        method: &str,
        mut params: Value,
        on_note: &mut impl FnMut(super::Notice<'_>),
    ) -> Result<(Option<Value>, Option<Value>)> {
        if self.broken {
            return Err(Error::Tool(format!(
                "`{method}` was not sent: an earlier request was cut off"
            )));
        }
        self.cancel_inflight().await;
        let id = json!(self.next);
        self.next += 1;
        if method == "tools/call"
            && let Some(object) = params.as_object_mut()
            && let Some(meta) = object
                .entry("_meta")
                .or_insert_with(|| json!({}))
                .as_object_mut()
        {
            meta.insert("progressToken".to_string(), id.clone());
        }
        let wait = self.wait;
        let deadline = Instant::now() + wait;
        self.inflight.push(id.clone());
        let sent = self
            .transport
            .send(&request(&id, method, params), deadline)
            .await;
        let result = match sent {
            Ok(Sent::Delivered) => self.reply(&id, method, deadline, on_note).await,
            Ok(Sent::Refused(error)) => Ok((None, Some(error))),
            Err(error) => Err(error),
            Ok(Sent::Late) => {
                self.broken = self.transport.piped();
                Err(no_reply(method, wait))
            }
        };
        self.inflight.retain(|pending| *pending != id);
        result
    }

    async fn give_up(&mut self, id: &Value, method: &str) -> Error {
        self.inflight.retain(|pending| pending != id);
        self.abandoned.push(id.clone());
        if method != "initialize" {
            self.cancel(id, "no reply in time").await;
        }
        no_reply(method, self.wait)
    }

    async fn frame(&mut self) -> Result<Read> {
        if self.broken {
            self.settle_stderr().await?;
            return Ok(Read::Closed);
        }
        let read = self.transport.next().await?;
        self.broken = self.transport.piped() && matches!(read, Read::Closed);
        Ok(read)
    }

    async fn write(&mut self, value: &Value) -> Result<()> {
        let deadline = Instant::now() + self.wait;
        match self.transport.send(value, deadline).await? {
            Sent::Delivered | Sent::Refused(_) => Ok(()),
            Sent::Late => {
                self.broken = self.transport.piped();
                Err(no_reply(
                    value
                        .get("method")
                        .and_then(Value::as_str)
                        .unwrap_or("reply"),
                    self.wait,
                ))
            }
        }
    }

    pub(crate) fn alive(&mut self) -> bool {
        match &mut self.transport {
            Transport::Stdio(stdio) => {
                !self.broken
                    && stdio
                        .child
                        .as_mut()
                        .is_some_and(|child| matches!(child.try_wait(), Ok(None)))
            }
            Transport::Http(http) => !http.expired(),
        }
    }

    fn draining(&self) -> bool {
        match &self.transport {
            Transport::Stdio(stdio) => stdio.stderr.draining(),
            Transport::Http(_) => false,
        }
    }

    fn abort_stderr(&self) {
        if let Transport::Stdio(stdio) = &self.transport {
            stdio.stderr.abort();
        }
    }

    async fn settle_stderr(&mut self) -> Result<()> {
        let Transport::Stdio(stdio) = &mut self.transport else {
            return Ok(());
        };
        if let Err(error) = stdio.stderr.settle().await
            && !error.is_cancelled()
        {
            return Err(Error::Tool(format!(
                "the child exited; stderr capture failed: {error}{}",
                stdio.note()
            )));
        }
        Ok(())
    }

    fn stderr_note(&self) -> String {
        match &self.transport {
            Transport::Stdio(stdio) => stdio.note(),
            Transport::Http(_) => String::new(),
        }
    }
}

fn no_reply(method: &str, wait: Duration) -> Error {
    Error::Tool(format!(
        "`{method}` got no reply in {}s; summonr cancelled it",
        wait.as_secs()
    ))
}

fn roots(cwd: &Path) -> Value {
    let uri = url::Url::from_directory_path(cwd)
        .map(|uri| uri.to_string())
        .unwrap_or_default();
    let name = cwd
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    json!([{"uri": uri, "name": name}])
}

#[cfg(all(test, unix))]
#[path = "client/process_tests.rs"]
mod tests;
