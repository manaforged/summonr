use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use url::Url;

use crate::error::{Error, Result};
use crate::language;
use crate::rpc;

use super::command::ProcessGroup;
use super::command::proc::kill_group;
use super::{ToolOutput, ToolRegistry, nonempty_path, object_bool, object_usize, parse_args};

const CONTENT_MODIFIED: i64 = -32801;
const INBOX: usize = 64;

pub(super) struct Conn {
    child: Option<Child>,
    inbox: mpsc::Receiver<Result<Value>>,
    reader: JoinHandle<()>,
    writer: Box<dyn AsyncWrite + Send + Unpin>,
    next_id: i64,
    root: String,
    clean: bool,
    docs: std::collections::HashMap<String, (String, i64)>,
    options: Value,
    server: String,
    key: String,
    group: Option<ProcessGroup>,
}

#[derive(Debug)]
pub(super) struct Launch {
    key: String,
    argv: Vec<String>,
    options: Value,
}

impl Conn {
    pub(super) async fn spawn(workspace: &Path, launch: &Launch) -> Result<Self> {
        let mut command = Command::new(&launch.argv[0]);
        command
            .args(&launch.argv[1..])
            .current_dir(workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .spawn()
            .map_err(|error| Error::Tool(format!("cannot start {}: {error}", launch.argv[0])))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Tool("language server has no stdout".to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error::Tool("language server has no stdin".to_string()))?;
        let (sender, inbox) = mpsc::channel(INBOX);
        let mut stdout = BufReader::new(stdout);
        let reader = tokio::spawn(async move {
            loop {
                let msg = read_msg(&mut stdout).await;
                let failed = msg.is_err();
                if !failed
                    && msg.as_ref().ok().is_some_and(|value| {
                        value.get("method").is_some() && value.get("id").is_none()
                    })
                {
                    continue;
                }
                if sender.send(msg).await.is_err() || failed {
                    break;
                }
            }
        });
        let group = child.id().map(|pid| ProcessGroup::new(pid, false));
        let mut conn = Self {
            child: Some(child),
            inbox,
            reader,
            writer: Box::new(stdin),
            next_id: 1,
            root: file_uri(workspace)?,
            clean: true,
            docs: std::collections::HashMap::new(),
            options: launch.options.clone(),
            server: launch.argv[0].clone(),
            key: launch.key.clone(),
            group,
        };
        conn.handshake(&launch.options).await?;
        Ok(conn)
    }

    async fn handshake(&mut self, options: &Value) -> Result<()> {
        let result = self
            .request(
                "initialize",
                json!({
                    "processId": null,
                    "rootUri": self.root,
                    "workspaceFolders": [{ "uri": self.root, "name": "workspace" }],
                    "initializationOptions": options,
                    "capabilities": {
                        "workspace": { "workspaceFolders": true, "configuration": true },
                        "window": { "workDoneProgress": true },
                        "textDocument": {
                            "publishDiagnostics": {},
                            "definition": {},
                            "references": {},
                            "rename": {},
                            "diagnostic": {}
                        }
                    }
                }),
            )
            .await?;
        drop(result);
        self.notify("initialized", json!({})).await?;
        Ok(())
    }

    pub(super) async fn diagnostics(&mut self, path: &Path) -> Result<(String, bool)> {
        self.did_open(path).await?;
        let uri = file_uri(path)?;
        let result = self
            .request(
                "textDocument/diagnostic",
                json!({ "textDocument": { "uri": uri } }),
            )
            .await?;
        let items = diagnostic_items(&result);
        if items.is_empty() {
            return Ok(("no diagnostics".to_string(), false));
        }
        Ok((items.join("\n"), has_error(&result)))
    }

    pub(super) async fn definition(
        &mut self,
        path: &Path,
        line: u32,
        character: u32,
    ) -> Result<String> {
        let result = self
            .at(path, line, character, "textDocument/definition")
            .await?;
        let locations = locations(&result);
        if locations.is_empty() {
            return Ok("no definition".to_string());
        }
        Ok(locations.join("\n"))
    }

    pub(super) async fn references(
        &mut self,
        path: &Path,
        line: u32,
        character: u32,
    ) -> Result<String> {
        self.did_open(path).await?;
        let uri = file_uri(path)?;
        let line = line.saturating_sub(1);
        let character = character.saturating_sub(1);
        let result = self
            .request(
                "textDocument/references",
                json!({
                    "textDocument": { "uri": uri },
                    "position": { "line": line, "character": character },
                    "context": { "includeDeclaration": true }
                }),
            )
            .await?;
        let locations = locations(&result);
        if locations.is_empty() {
            return Ok("no references".to_string());
        }
        Ok(locations.join("\n"))
    }

    pub(super) async fn rename(
        &mut self,
        path: &Path,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Result<Value> {
        self.did_open(path).await?;
        let uri = file_uri(path)?;
        let line = line.saturating_sub(1);
        let character = character.saturating_sub(1);
        self.request(
            "textDocument/rename",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                "newName": new_name
            }),
        )
        .await
    }

    async fn at(&mut self, path: &Path, line: u32, character: u32, method: &str) -> Result<Value> {
        self.did_open(path).await?;
        let uri = file_uri(path)?;
        let line = line.saturating_sub(1);
        let character = character.saturating_sub(1);
        self.request(
            method,
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )
        .await
    }

    async fn did_open(&mut self, path: &Path) -> Result<()> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| Error::Tool(format!("cannot read {}: {error}", path.display())))?;
        let uri = file_uri(path)?;
        let language_id = language::language_id(path);
        match self.docs.get_mut(&uri) {
            Some((held, _)) if *held == text => Ok(()),
            Some((held, version)) => {
                *version += 1;
                *held = text.clone();
                let version = *version;
                self.notify(
                    "textDocument/didChange",
                    json!({
                        "textDocument": { "uri": uri, "version": version },
                        "contentChanges": [{ "text": text }]
                    }),
                )
                .await
            }
            None => {
                self.docs.insert(uri.clone(), (text.clone(), 1));
                self.notify(
                    "textDocument/didOpen",
                    json!({
                        "textDocument": {
                            "uri": uri,
                            "languageId": language_id,
                            "version": 1,
                            "text": text
                        }
                    }),
                )
                .await
            }
        }
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let mut reply = self.call(method, params.clone()).await?;
        if reply.pointer("/error/code").and_then(Value::as_i64) == Some(CONTENT_MODIFIED) {
            reply = self.call(method, params).await?;
        }
        rpc::outcome(
            "lsp",
            reply.get("result").cloned(),
            reply.get("error").cloned(),
        )
    }

    async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&rpc::request(&json!(id), method, params)).await?;
        self.pump(id).await
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.send(&rpc::notification(method, params)).await
    }

    async fn send(&mut self, value: &Value) -> Result<()> {
        self.clean = false;
        write_msg(&mut self.writer, value).await?;
        self.clean = true;
        Ok(())
    }

    async fn pump(&mut self, want: i64) -> Result<Value> {
        loop {
            let msg = match self.inbox.recv().await {
                Some(Ok(msg)) => msg,
                Some(Err(error)) => {
                    self.clean = false;
                    return Err(error);
                }
                None => {
                    self.clean = false;
                    return Err(Error::Tool("language server closed".to_string()));
                }
            };
            if msg.get("method").is_some() && msg.get("id").is_some() {
                self.reply_server(&msg).await?;
                continue;
            }
            if msg.get("id") == Some(&Value::from(want)) {
                return Ok(msg);
            }
        }
    }

    async fn reply_server(&mut self, msg: &Value) -> Result<()> {
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let result = match method {
            "workspace/workspaceFolders" => json!([{ "uri": self.root, "name": "workspace" }]),
            "workspace/configuration" => Value::Array(
                params
                    .get("items")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .map(|item| {
                                let section = item.get("section").and_then(Value::as_str);
                                if section
                                    .is_some_and(|name| name != self.server && name != self.key)
                                {
                                    Value::Null
                                } else {
                                    self.options.clone()
                                }
                            })
                            .collect()
                    })
                    .unwrap_or_else(|| vec![self.options.clone()]),
            ),
            "window/workDoneProgress/create" | "client/registerCapability" => Value::Null,
            _ => Value::Null,
        };
        self.send(&rpc::response(&id, result)).await
    }

    fn kill(&mut self) {
        self.reader.abort();
        drop(self.group.take());
        if let Some(child) = self.child.as_mut() {
            if let Some(pid) = child.id() {
                kill_group(pid, false);
            }
            drop(child.start_kill());
        }
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        self.kill();
    }
}

mod detect;
mod proto;
mod tool;

use detect::Scope;
pub(in crate::tools) use detect::{Launches, available};
use proto::*;
