use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::time::Instant;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};

use super::{Notice, agreed, hello, notification, request};
use crate::error::{Error, Result};
use crate::limits::{MCP_CANCEL_WRITE, MCP_FRAME_MAX, MCP_STDERR_TAIL};
use crate::tools::command::ProcessGroup;
use crate::tools::sandbox::Sandbox;

mod reply;

const SECRET_MIN: usize = 8;

pub(crate) struct Client {
    _group: Option<ProcessGroup>,
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    pending: Vec<u8>,
    over: bool,
    abandoned: Vec<Value>,
    inflight: Vec<Value>,
    roots: Value,
    broken: bool,
    stderr: Arc<Mutex<Vec<u8>>>,
    drain: Option<tokio::task::JoinHandle<()>>,
    secrets: Vec<String>,
    wait: Duration,
    next: i64,
}

enum Read {
    Line(String),
    Oversize,
    Closed,
}

pub(crate) struct Spawn<'a> {
    pub(crate) program: &'a str,
    pub(crate) args: &'a [&'a str],
    pub(crate) env: &'a [(String, String)],
    pub(crate) cwd: &'a Path,
    pub(crate) sandbox: Option<&'a Sandbox>,
    pub(crate) wait: Duration,
}

impl Client {
    pub(crate) fn spawn(launch: &Spawn<'_>) -> Result<Self> {
        let mut command = match launch.sandbox {
            Some(sandbox) => sandbox.command(launch.program, launch.args, launch.cwd),
            None => {
                let mut command = Command::new(launch.program);
                command.args(launch.args).current_dir(launch.cwd);
                command
            }
        };
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .envs(launch.env.iter().map(|(key, value)| (key, value)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| Error::Tool(format!("cannot start `{}`: {error}", launch.program)))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Tool("child has no stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Tool("child has no stdout".to_string()))?;
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let drain = child
            .stderr
            .take()
            .map(|pipe| tokio::spawn(tail(pipe, Arc::clone(&stderr))));
        let _group = child
            .id()
            .map(|pid| ProcessGroup::new(pid, launch.sandbox.is_some()));
        Ok(Self {
            _group,
            child,
            stdin,
            reader: BufReader::new(stdout),
            pending: Vec::new(),
            over: false,
            abandoned: Vec::new(),
            inflight: Vec::new(),
            roots: roots(launch.cwd),
            broken: false,
            stderr,
            drain,
            secrets: launch
                .env
                .iter()
                .map(|(_, value)| value.clone())
                .filter(|value| value.len() >= SECRET_MIN)
                .collect(),
            wait: launch.wait,
            next: 1,
        })
    }

    pub(crate) fn set_wait(&mut self, wait: Duration) {
        self.wait = wait;
    }

    pub(crate) async fn initialize(&mut self, name: &str) -> Result<Value> {
        let capabilities = json!({"roots": {"listChanged": false}, "elicitation": {"form": {}}});
        let result = self
            .call("initialize", hello(name, capabilities), |_| {})
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
            self.send(&line, Instant::now() + MCP_CANCEL_WRITE).await,
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
            self.stdin.write_all(line.as_bytes()).await?;
            self.stdin.flush().await
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
        loop {
            let chunk = self.reader.fill_buf().await?;
            if chunk.is_empty() {
                self.broken = true;
                return Ok(Read::Closed);
            }
            let (take, done) = chunk
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or((chunk.len(), false), |at| (at + 1, true));
            if !self.over && self.pending.len() + take <= MCP_FRAME_MAX {
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

    async fn write(&mut self, value: &Value) -> Result<()> {
        let line = frame_line(value)?;
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await?;
        Ok(())
    }

    pub(crate) fn alive(&mut self) -> bool {
        !self.broken && matches!(self.child.try_wait(), Ok(None))
    }

    async fn settle_stderr(&mut self) -> Result<()> {
        if let Some(drain) = self.drain.as_mut()
            && let Err(error) = drain.await
            && !error.is_cancelled()
        {
            self.drain = None;
            return Err(Error::Tool(format!(
                "the child exited; stderr capture failed: {error}{}",
                self.stderr_note()
            )));
        }
        self.drain = None;
        Ok(())
    }

    fn stderr_note(&self) -> String {
        let bytes = self
            .stderr
            .lock()
            .map(|bytes| bytes.clone())
            .unwrap_or_default();
        let text = String::from_utf8_lossy(&bytes);
        text.lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .map(|line| {
                format!(
                    "; stderr: {}",
                    crate::text::redact(line.trim(), self.secrets.iter().map(String::as_str))
                )
            })
            .unwrap_or_default()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        if let Some(drain) = &self.drain {
            drain.abort();
        }
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

async fn tail(mut pipe: ChildStderr, keep: Arc<Mutex<Vec<u8>>>) {
    let mut chunk = [0_u8; 1024];
    while let Ok(read) = pipe.read(&mut chunk).await {
        if read == 0 {
            break;
        }
        let Ok(mut bytes) = keep.lock() else {
            break;
        };
        bytes.extend_from_slice(&chunk[..read]);
        let excess = bytes.len().saturating_sub(MCP_STDERR_TAIL);
        bytes.drain(..excess);
    }
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
