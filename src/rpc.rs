use std::path::Path;
use std::process::Stdio;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

use crate::error::{Error, Result};

pub(crate) const PROTOCOL: &str = "2025-03-26";

/// One newline-delimited JSON-RPC 2.0 frame.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Frame {
    Request {
        id: Value,
        method: String,
        params: Value,
    },
    Notification {
        method: String,
        params: Value,
    },
    Response {
        id: Value,
        result: Option<Value>,
        error: Option<Value>,
    },
}

pub(crate) fn parse(line: &str) -> Result<Frame> {
    let value: Value = serde_json::from_str(line)?;
    let method = value.get("method").and_then(Value::as_str);
    let id = value.get("id").cloned();
    let params = value.get("params").cloned().unwrap_or(Value::Null);
    match (method, id) {
        (Some(method), Some(id)) if !id.is_null() => Ok(Frame::Request {
            id,
            method: method.to_string(),
            params,
        }),
        (Some(method), _) => Ok(Frame::Notification {
            method: method.to_string(),
            params,
        }),
        (None, Some(id)) => Ok(Frame::Response {
            id,
            result: value.get("result").cloned(),
            error: value.get("error").cloned(),
        }),
        (None, None) => Err(Error::Tool("frame has neither method nor id".to_string())),
    }
}

pub(crate) fn response(id: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

pub(crate) fn failure(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

pub(crate) fn notification(method: &str, params: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": method, "params": params})
}

pub(crate) fn text(result: &Value) -> String {
    result
        .get("content")
        .and_then(Value::as_array)
        .map(|parts| {
            parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

/// A JSON-RPC client over a child process's stdio, one request at a time.
pub(crate) struct Client {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    next: i64,
}

impl Client {
    pub(crate) fn spawn(program: &str, args: &[&str], cwd: &Path) -> Result<Self> {
        let mut child = Command::new(program)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| Error::Tool(format!("cannot start `{program}`: {error}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Tool("child has no stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Tool("child has no stdout".to_string()))?;
        Ok(Self {
            child,
            stdin,
            lines: BufReader::new(stdout).lines(),
            next: 1,
        })
    }

    pub(crate) async fn initialize(&mut self, name: &str) -> Result<Value> {
        let result = self
            .call(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL,
                    "capabilities": {},
                    "clientInfo": {"name": name, "version": env!("CARGO_PKG_VERSION")}
                }),
                |_, _| {},
            )
            .await?;
        self.notify("notifications/initialized", json!({})).await?;
        Ok(result)
    }

    pub(crate) async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.write(&notification(method, params)).await
    }

    pub(crate) async fn tool(
        &mut self,
        name: &str,
        arguments: Value,
        on_note: impl FnMut(&str, &Value),
    ) -> Result<Value> {
        self.call(
            "tools/call",
            json!({"name": name, "arguments": arguments}),
            on_note,
        )
        .await
    }

    pub(crate) async fn call(
        &mut self,
        method: &str,
        params: Value,
        mut on_note: impl FnMut(&str, &Value),
    ) -> Result<Value> {
        let id = json!(self.next);
        self.next += 1;
        self.write(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await?;
        loop {
            let Some(line) = self.lines.next_line().await? else {
                return Err(Error::Tool(format!("the child exited during `{method}`")));
            };
            if line.trim().is_empty() {
                continue;
            }
            let Ok(frame) = parse(&line) else {
                continue;
            };
            match frame {
                Frame::Response {
                    id: got,
                    result,
                    error,
                } if got == id => {
                    if let Some(error) = error {
                        let message = error
                            .get("message")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown error");
                        return Err(Error::Tool(format!("`{method}` failed: {message}")));
                    }
                    return Ok(result.unwrap_or(Value::Null));
                }
                Frame::Response { .. } => {}
                Frame::Notification { method, params } => on_note(&method, &params),
                Frame::Request { id, method, .. } => {
                    self.write(&failure(&id, -32601, &format!("{method} is not supported")))
                        .await?;
                }
            }
        }
    }

    async fn write(&mut self, value: &Value) -> Result<()> {
        let mut line = serde_json::to_string(value)?;
        line.push('\n');
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await?;
        Ok(())
    }

    pub(crate) async fn shutdown(mut self) {
        drop(self.stdin);
        drop(self.child.kill().await);
    }
}

#[cfg(test)]
#[path = "rpc_tests.rs"]
mod tests;
