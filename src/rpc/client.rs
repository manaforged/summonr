use std::path::Path;
use std::time::Duration;

use tokio::time::Instant;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout};

use super::{Notice, agreed, hello, notification, request};
use crate::error::{Error, Result};
use crate::process::{Cap, Child, Env, Spec, Tail};
use crate::tools::command::ProcessGroup;

mod reply;

pub(crate) struct Client {
    group: Option<ProcessGroup>,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    reader: Lines<ChildStdout>,
    abandoned: Vec<Value>,
    inflight: Vec<Value>,
    roots: Value,
    broken: bool,
    stderr: Tail,
    secrets: Vec<String>,
    wait: Duration,
    next: i64,
}

pub(crate) enum Read {
    Line(String),
    Oversize,
    Closed,
}

pub(crate) struct Lines<R> {
    reader: BufReader<R>,
    pending: Vec<u8>,
    over: bool,
}

impl<R: AsyncRead + Unpin> Lines<R> {
    pub(crate) fn new(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
            pending: Vec::new(),
            over: false,
        }
    }

    pub(crate) async fn next(&mut self) -> std::io::Result<Read> {
        loop {
            let chunk = self.reader.fill_buf().await?;
            if chunk.is_empty() {
                return Ok(Read::Closed);
            }
            let (take, done) = chunk
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or((chunk.len(), false), |at| (at + 1, true));
            if !self.over
                && self.pending.len() + take <= crate::limits::limits().mcp_frame_max_bytes
            {
                self.pending.extend_from_slice(&chunk[..take]);
            } else if !self.over {
                self.over = true;
                self.pending = Vec::new();
            }
            self.reader.consume(take);
            if done {
                break;
            }
        }
        let line = std::mem::take(&mut self.pending);
        if std::mem::take(&mut self.over) {
            return Ok(Read::Oversize);
        }
        Ok(Read::Line(String::from_utf8(line).unwrap_or_else(
            |error| String::from_utf8_lossy(error.as_bytes()).into_owned(),
        )))
    }
}

pub(crate) struct Spawn<'a> {
    pub(crate) program: &'a str,
    pub(crate) args: &'a [&'a str],
    pub(crate) env: &'a [(String, String)],
    pub(crate) cwd: &'a Path,
    pub(crate) wait: Duration,
}

impl Client {
    pub(crate) fn spawn(launch: &Spawn<'_>) -> Result<Self> {
        let process = crate::process::start(
            Spec {
                cwd: Some(launch.cwd.to_path_buf()),
                base: Env::Clean,
                ..Spec::new(launch.program, launch.args)
            }
            .envs(launch.env.iter().cloned()),
        )?;
        let stderr = Tail::spawn(
            process.stderr,
            Cap::Tail(crate::limits::limits().mcp_stderr_tail_bytes),
        );
        Ok(Self {
            group: process.group,
            child: Some(process.child),
            stdin: Some(process.stdin),
            reader: Lines::new(process.stdout),
            abandoned: Vec::new(),
            inflight: Vec::new(),
            roots: roots(launch.cwd),
            broken: false,
            stderr,
            secrets: launch.env.iter().map(|(_, value)| value.clone()).collect(),
            wait: launch.wait,
            next: 1,
        })
    }

    pub(crate) fn pid(&self) -> Option<u32> {
        self.child.as_ref().and_then(Child::id)
    }

    pub(crate) fn set_wait(&mut self, wait: Duration) {
        self.wait = wait;
    }

    pub(crate) async fn initialize(
        &mut self,
        name: &str,
        on_note: impl FnMut(Notice<'_>),
    ) -> Result<Value> {
        let capabilities = json!({"roots": {"listChanged": false}, "elicitation": {"form": {}}});
        let result = self
            .call("initialize", hello(name, capabilities), on_note)
            .await?;
        agreed(&result)?;
        self.notify("notifications/initialized", json!({})).await?;
        Ok(result)
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
        let cancel = notification(
            "notifications/cancelled",
            json!({"requestId": id, "reason": reason}),
        );
        let Ok(line) = frame_line(&cancel) else {
            return;
        };
        if !matches!(
            self.send(
                &line,
                Instant::now() + crate::limits::limits().mcp_cancel_write()
            )
            .await,
            Ok(Ok(()))
        ) {
            self.broken = true;
        }
    }

    async fn send(
        &mut self,
        line: &str,
        deadline: Instant,
    ) -> std::result::Result<std::io::Result<()>, tokio::time::error::Elapsed> {
        tokio::time::timeout_at(deadline, async {
            let pipe = self.pipe()?;
            pipe.write_all(line.as_bytes()).await?;
            pipe.flush().await
        })
        .await
    }

    pub(crate) async fn call(
        &mut self,
        method: &str,
        mut params: Value,
        mut on_note: impl FnMut(Notice<'_>),
    ) -> Result<Value> {
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
        {
            object.insert("_meta".to_string(), json!({"progressToken": id}));
        }
        let line = frame_line(&request(&id, method, params))?;
        let wait = self.wait;
        let deadline = Instant::now() + wait;
        self.inflight.push(id.clone());
        let written = self.send(&line, deadline).await;
        let result = match written {
            Ok(Ok(())) => self.reply(&id, method, deadline, &mut on_note).await,
            Ok(Err(error)) => Err(error.into()),
            Err(_elapsed) => {
                self.broken = true;
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
        let read = self.reader.next().await?;
        self.broken = matches!(read, Read::Closed);
        Ok(read)
    }

    async fn write(&mut self, value: &Value) -> Result<()> {
        let line = frame_line(value)?;
        let pipe = self.pipe()?;
        pipe.write_all(line.as_bytes()).await?;
        pipe.flush().await?;
        Ok(())
    }

    fn pipe(&mut self) -> std::io::Result<&mut ChildStdin> {
        self.stdin
            .as_mut()
            .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::BrokenPipe))
    }

    pub(crate) fn alive(&mut self) -> bool {
        !self.broken
            && self
                .child
                .as_mut()
                .is_some_and(|child| matches!(child.try_wait(), Ok(None)))
    }

    async fn settle_stderr(&mut self) -> Result<()> {
        if let Err(error) = self.stderr.settle().await
            && !error.is_cancelled()
        {
            return Err(Error::Tool(format!(
                "the child exited; stderr capture failed: {error}{}",
                self.stderr_note()
            )));
        }
        Ok(())
    }

    fn stderr_note(&self) -> String {
        self.stderr
            .last_line(&self.secrets)
            .map(|line| format!("; stderr: {line}"))
            .unwrap_or_default()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let (Some(child), group) = (self.child.take(), self.group.take()) else {
            return;
        };
        crate::supervise::close(&runtime, child, group);
    }
}

fn no_reply(method: &str, wait: Duration) -> Error {
    Error::Tool(format!(
        "`{method}` got no reply in {}s; summonr cancelled it",
        wait.as_secs()
    ))
}

fn frame_line(value: &Value) -> Result<String> {
    let mut line = serde_json::to_string(value)?;
    line.push('\n');
    Ok(line)
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
