use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use url::Url;

use crate::error::{Error, Result};
use crate::language;
use crate::rpc;

use super::{ToolOutput, ToolRegistry, nonempty_path, object_bool, object_usize, parse_args};

const LSP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub(super) struct Conn {
    child: Option<Child>,
    reader: BufReader<Box<dyn AsyncRead + Send + Unpin>>,
    writer: Box<dyn AsyncWrite + Send + Unpin>,
    next_id: i64,
    root: String,
}

#[derive(Debug)]
pub(super) struct Launch {
    key: String,
    argv: Vec<String>,
    options: Value,
}

impl Conn {
    pub(super) async fn spawn(workspace: &Path, launch: &Launch) -> Result<Self> {
        let mut child = Command::new(&launch.argv[0])
            .args(&launch.argv[1..])
            .current_dir(workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
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
        let mut conn = Self {
            child: Some(child),
            reader: BufReader::new(Box::new(stdout)),
            writer: Box::new(stdin),
            next_id: 1,
            root: file_uri(workspace)?,
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

    pub(super) async fn diagnostics(&mut self, path: &Path) -> Result<String> {
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
            return Ok("no diagnostics".to_string());
        }
        Ok(items.join("\n"))
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
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": file_uri(path)?,
                    "languageId": language::language_id(path),
                    "version": 1,
                    "text": text
                }
            }),
        )
        .await
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        write_msg(&mut self.writer, &rpc::request(&json!(id), method, params)).await?;
        self.pump(id).await
    }

    async fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        write_msg(&mut self.writer, &rpc::notification(method, params)).await
    }

    async fn pump(&mut self, want: i64) -> Result<Value> {
        loop {
            let msg = read_msg(&mut self.reader).await?;
            if msg.get("method").is_some() && msg.get("id").is_some() {
                self.reply_server(&msg).await?;
                continue;
            }
            if msg.get("id") == Some(&Value::from(want)) {
                return rpc::outcome("lsp", msg.get("result").cloned(), msg.get("error").cloned());
            }
        }
    }

    async fn reply_server(&mut self, msg: &Value) -> Result<()> {
        let id = msg.get("id").cloned().unwrap_or(Value::Null);
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or(Value::Null);
        let result = match method {
            "workspace/workspaceFolders" => json!([{ "uri": self.root, "name": "workspace" }]),
            "workspace/configuration" => {
                let n = params
                    .get("items")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(1);
                Value::Array(vec![Value::Null; n])
            }
            "window/workDoneProgress/create" | "client/registerCapability" => Value::Null,
            _ => Value::Null,
        };
        write_msg(&mut self.writer, &rpc::response(&id, result)).await
    }

    fn kill(&mut self) {
        if let Some(child) = self.child.as_mut() {
            drop(child.start_kill());
        }
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        self.kill();
    }
}

pub(super) fn detect(workspace: &Path, path: &Path) -> Result<Launch> {
    if let Some(module) = workspace_module(workspace)? {
        return built(language::named(&module)?);
    }
    let config = crate::config::load()?;
    if let Some(command) = config.lsp.command {
        return custom(&command);
    }
    if let Some(module) = config.lsp.module {
        return built(language::named(&module)?);
    }
    from_tree(workspace, path)
}

pub(super) fn available(workspace: &Path) -> bool {
    if let Ok(Some(module)) = workspace_module(workspace) {
        return language::named(&module).is_ok_and(|module| built(module).is_ok());
    }
    if let Ok(config) = crate::config::load() {
        if config.lsp.command.is_some() {
            return true;
        }
        if let Some(module) = config.lsp.module {
            return language::named(&module).is_ok_and(|module| built(module).is_ok());
        }
    }
    language::present(workspace)
        .into_iter()
        .any(|module| built(module).is_ok())
}

fn from_tree(workspace: &Path, path: &Path) -> Result<Launch> {
    built(language::detect(workspace, path)?)
}

fn workspace_module(workspace: &Path) -> Result<Option<String>> {
    let path = workspace.join(".mage/config.toml");
    if !path.is_file() {
        return Ok(None);
    }
    let config = crate::config::load_from(&path)?;
    if config.lsp.command.is_some() {
        return Err(Error::Config(
            "workspace .mage/config.toml cannot set [lsp] command; use a curated module or put the command in ~/.mage/config.toml"
                .to_string(),
        ));
    }
    Ok(config.lsp.module)
}

fn built(module: &language::Module) -> Result<Launch> {
    let mut names = Vec::new();
    for server in module.servers {
        names.push(server.name);
        if on_path(server.name) {
            let mut command = vec![server.name.to_string()];
            command.extend(server.args.iter().map(|arg| (*arg).to_string()));
            return Ok(Launch {
                key: module.id.to_string(),
                argv: command,
                options: language::options(module),
            });
        }
    }
    Err(Error::Tool(format!(
        "{} not on PATH for the {} module; install one or set [lsp] command in ~/.mage/config.toml",
        names.join(" or "),
        module.id
    )))
}

fn custom(command: &[String]) -> Result<Launch> {
    Ok(Launch {
        key: "custom".to_string(),
        argv: argv(command)?,
        options: json!({}),
    })
}

fn argv(command: &[String]) -> Result<Vec<String>> {
    let name = command
        .first()
        .map(String::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| Error::Tool("[lsp] command is empty".to_string()))?;
    if !on_path(name) {
        return Err(Error::Tool(format!(
            "{name} is not on PATH; fix [lsp] command in ~/.mage/config.toml"
        )));
    }
    Ok(command.to_vec())
}

fn on_path(name: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        dir.join(name).is_file()
            || (cfg!(windows)
                && Path::new(name).extension().is_none()
                && dir.join(format!("{name}.exe")).is_file())
    })
}

fn file_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(String::from)
        .map_err(|()| Error::Tool(format!("cannot convert {} to a file URI", path.display())))
}

fn diagnostic_items(value: &Value) -> Vec<String> {
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| value.as_array().cloned())
        .unwrap_or_default();
    items.iter().filter_map(format_diagnostic).collect()
}

fn format_diagnostic(item: &Value) -> Option<String> {
    let message = item.get("message")?.as_str()?.trim();
    if message.is_empty() {
        return None;
    }
    let severity = match item.get("severity").and_then(Value::as_u64).unwrap_or(1) {
        1 => "error",
        2 => "warning",
        3 => "info",
        _ => "hint",
    };
    let line = item
        .pointer("/range/start/line")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        + 1;
    let col = item
        .pointer("/range/start/character")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        + 1;
    Some(format!("{severity} {line}:{col}: {message}"))
}

fn locations(value: &Value) -> Vec<String> {
    let list = if let Some(array) = value.as_array() {
        array.clone()
    } else if value.is_object() {
        vec![value.clone()]
    } else {
        return Vec::new();
    };
    list.iter().filter_map(format_location).collect()
}

struct TextEdit {
    start_line: usize,
    start_col: usize,
    end_line: usize,
    end_col: usize,
    new_text: String,
}

enum Change {
    Text { uri: String, edits: Vec<TextEdit> },
    Create { uri: String },
    Rename { from: String, to: String },
    Delete { uri: String },
}

fn collect_changes(edit: &Value) -> Result<Vec<Change>> {
    let mut ops = Vec::new();
    if let Some(changes) = edit.get("changes").and_then(Value::as_object) {
        for (uri, value) in changes {
            let edits = parse_edits(value)?;
            if !edits.is_empty() {
                ops.push(Change::Text {
                    uri: uri.clone(),
                    edits,
                });
            }
        }
    }
    if let Some(docs) = edit.get("documentChanges").and_then(Value::as_array) {
        for item in docs {
            match item.get("kind").and_then(Value::as_str) {
                Some("create") => {
                    let uri = item
                        .get("uri")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::Tool("lsp create is missing a uri".to_string()))?;
                    ops.push(Change::Create {
                        uri: uri.to_string(),
                    });
                }
                Some("rename") => {
                    let from = item
                        .get("oldUri")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::Tool("lsp rename is missing oldUri".to_string()))?;
                    let to = item
                        .get("newUri")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::Tool("lsp rename is missing newUri".to_string()))?;
                    ops.push(Change::Rename {
                        from: from.to_string(),
                        to: to.to_string(),
                    });
                }
                Some("delete") => {
                    let uri = item
                        .get("uri")
                        .and_then(Value::as_str)
                        .ok_or_else(|| Error::Tool("lsp delete is missing a uri".to_string()))?;
                    ops.push(Change::Delete {
                        uri: uri.to_string(),
                    });
                }
                Some(other) => {
                    return Err(Error::Tool(format!(
                        "lsp rename includes unsupported file operation `{other}`"
                    )));
                }
                None => {
                    let uri = item
                        .pointer("/textDocument/uri")
                        .and_then(Value::as_str)
                        .ok_or_else(|| {
                            Error::Tool("lsp rename is missing a document uri".to_string())
                        })?;
                    let edits = item
                        .get("edits")
                        .map(parse_edits)
                        .transpose()?
                        .unwrap_or_default();
                    if !edits.is_empty() {
                        ops.push(Change::Text {
                            uri: uri.to_string(),
                            edits,
                        });
                    }
                }
            }
        }
    }
    Ok(ops)
}

fn parse_edits(value: &Value) -> Result<Vec<TextEdit>> {
    let Some(items) = value.as_array() else {
        return Ok(Vec::new());
    };
    items.iter().map(parse_edit).collect()
}

fn parse_edit(item: &Value) -> Result<TextEdit> {
    let new_text = item
        .get("newText")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Tool("lsp edit is missing newText".to_string()))?;
    let start_line = item
        .pointer("/range/start/line")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::Tool("lsp edit is missing a range".to_string()))?;
    let start_col = item
        .pointer("/range/start/character")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::Tool("lsp edit is missing a range".to_string()))?;
    let end_line = item
        .pointer("/range/end/line")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::Tool("lsp edit is missing a range".to_string()))?;
    let end_col = item
        .pointer("/range/end/character")
        .and_then(Value::as_u64)
        .ok_or_else(|| Error::Tool("lsp edit is missing a range".to_string()))?;
    Ok(TextEdit {
        start_line: start_line as usize,
        start_col: start_col as usize,
        end_line: end_line as usize,
        end_col: end_col as usize,
        new_text: new_text.to_string(),
    })
}

fn apply_text_edits(text: &str, edits: &[TextEdit]) -> Result<String> {
    let mut ordered: Vec<&TextEdit> = edits.iter().collect();
    ordered.sort_by_key(|edit| (edit.start_line, edit.start_col, edit.end_line, edit.end_col));
    for pair in ordered.windows(2) {
        let left = pair[0];
        let right = pair[1];
        if (left.end_line, left.end_col) > (right.start_line, right.start_col) {
            return Err(Error::Tool(
                "lsp rename edits overlap — apply them one at a time".to_string(),
            ));
        }
    }
    let mut out = text.to_string();
    for edit in ordered.iter().rev() {
        let start = offset(&out, edit.start_line, edit.start_col)?;
        let end = offset(&out, edit.end_line, edit.end_col)?;
        if start > end {
            return Err(Error::Tool("lsp edit range is inverted".to_string()));
        }
        out.replace_range(start..end, &edit.new_text);
    }
    Ok(out)
}

fn offset(text: &str, line: usize, character: usize) -> Result<usize> {
    let mut pos = 0usize;
    let mut rest = text;
    for _ in 0..line {
        let Some(index) = rest.find('\n') else {
            return Err(Error::Tool(
                "lsp edit line is past the end of the file".to_string(),
            ));
        };
        pos += index + 1;
        rest = &rest[index + 1..];
    }
    let row = rest.split('\n').next().unwrap_or(rest);
    let mut units = 0usize;
    for (index, ch) in row.char_indices() {
        if units == character {
            return Ok(pos + index);
        }
        if units > character {
            return Err(Error::Tool(
                "lsp edit column splits a UTF-16 surrogate pair".to_string(),
            ));
        }
        units += ch.len_utf16();
    }
    if units == character {
        return Ok(pos + row.len());
    }
    Err(Error::Tool(
        "lsp edit column is past the end of the line".to_string(),
    ))
}

fn format_location(item: &Value) -> Option<String> {
    let uri = item
        .get("uri")
        .or_else(|| item.get("targetUri"))
        .and_then(Value::as_str)?;
    let line = item
        .pointer("/range/start/line")
        .or_else(|| item.pointer("/targetRange/start/line"))
        .or_else(|| item.pointer("/targetSelectionRange/start/line"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
        + 1;
    let col = item
        .pointer("/range/start/character")
        .or_else(|| item.pointer("/targetRange/start/character"))
        .or_else(|| item.pointer("/targetSelectionRange/start/character"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
        + 1;
    let path = uri
        .strip_prefix("file://")
        .unwrap_or(uri)
        .replace("%20", " ");
    Some(format!("{path}:{line}:{col}"))
}

pub(super) async fn write_msg(writer: &mut (impl AsyncWrite + Unpin), value: &Value) -> Result<()> {
    let body = serde_json::to_vec(value)?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer
        .write_all(header.as_bytes())
        .await
        .map_err(|error| Error::Tool(format!("lsp write: {error}")))?;
    writer
        .write_all(&body)
        .await
        .map_err(|error| Error::Tool(format!("lsp write: {error}")))?;
    writer
        .flush()
        .await
        .map_err(|error| Error::Tool(format!("lsp write: {error}")))?;
    Ok(())
}

pub(super) async fn read_msg(reader: &mut BufReader<impl AsyncRead + Unpin>) -> Result<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
            .await
            .map_err(|error| Error::Tool(format!("lsp read: {error}")))?;
        if n == 0 {
            return Err(Error::Tool("language server closed".to_string()));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if let Some(value) = line
            .to_ascii_lowercase()
            .strip_prefix("content-length:")
            .map(str::trim)
        {
            length = value.parse().ok();
        }
    }
    let length =
        length.ok_or_else(|| Error::Tool("lsp message missing Content-Length".to_string()))?;
    let mut body = vec![0_u8; length];
    reader
        .read_exact(&mut body)
        .await
        .map_err(|error| Error::Tool(format!("lsp read: {error}")))?;
    serde_json::from_slice(&body).map_err(|error| Error::Tool(format!("lsp json: {error}")))
}

impl ToolRegistry {
    pub(super) async fn lsp(&self, arguments: &str) -> Result<ToolOutput> {
        let args = parse_args("lsp", arguments)?;
        match args.get("op").and_then(Value::as_str) {
            Some("diagnostics") => return self.diagnostics(arguments).await,
            Some("definition") => return self.definition(arguments).await,
            Some(operation) => {
                return Err(Error::Tool(format!("unknown lsp operation `{operation}`")));
            }
            None => {}
        }
        if args.get("line").is_some()
            || args.get("character").is_some()
            || args.get("references").and_then(Value::as_bool) == Some(true)
            || args
                .get("new_name")
                .and_then(Value::as_str)
                .is_some_and(|name| !name.trim().is_empty())
        {
            return self.definition(arguments).await;
        }
        self.diagnostics(arguments).await
    }

    pub(super) async fn diagnostics(&self, arguments: &str) -> Result<ToolOutput> {
        let args = parse_args("diagnostics", arguments)?;
        let path = nonempty_path(&args, "diagnostics")?;
        let file = self.resolve_existing(path)?;
        let request_path = file.clone();
        let text = self
            .with_lsp(&file, move |conn| {
                Box::pin(async move { conn.diagnostics(&request_path).await })
            })
            .await?;
        Ok(ToolOutput::ok(text))
    }

    pub(super) async fn definition(&self, arguments: &str) -> Result<ToolOutput> {
        let args = parse_args("definition", arguments)?;
        let path = nonempty_path(&args, "definition")?;
        let file = self.resolve_existing(path)?;
        let line = object_usize(&args, "line").unwrap_or(1).max(1) as u32;
        let character = object_usize(&args, "character").unwrap_or(1).max(1) as u32;
        let new_name = args
            .get("new_name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty());
        if let Some(new_name) = new_name {
            if self.policy.is_read_only() {
                return Err(Error::Denied(
                    "plan mode is read-only — no rename".to_string(),
                ));
            }
            let name = new_name.to_string();
            let request_path = file.clone();
            let edit = self
                .with_lsp(&file, move |conn| {
                    Box::pin(
                        async move { conn.rename(&request_path, line, character, &name).await },
                    )
                })
                .await?;
            return self.apply_workspace_edit(&edit);
        }
        let refs = object_bool(&args, "references");
        let request_path = file.clone();
        let text = self
            .with_lsp(&file, move |conn| {
                Box::pin(async move {
                    if refs {
                        conn.references(&request_path, line, character).await
                    } else {
                        conn.definition(&request_path, line, character).await
                    }
                })
            })
            .await?;
        Ok(ToolOutput::ok(text))
    }

    pub(super) async fn check_after_edit(&self, dest: &Path) -> Option<ToolOutput> {
        let key = detect(&self.workspace, dest).ok()?.key;
        let slot = self.lsp.lock().ok()?.get(&key).cloned()?;
        if slot.lock().await.is_none() {
            return None;
        }
        let request_path = dest.to_path_buf();
        let text = self
            .with_lsp(dest, move |conn| {
                Box::pin(async move { conn.diagnostics(&request_path).await })
            })
            .await
            .ok()?;
        Some(ToolOutput {
            text: format!("diagnostics: {text}"),
            success: !text.starts_with("error "),
            mutated: false,
            grant: None,
        })
    }

    pub(super) fn apply_workspace_edit(&self, edit: &Value) -> Result<ToolOutput> {
        if edit.is_null() {
            return Err(Error::Tool(
                "language server has no rename for this symbol".to_string(),
            ));
        }
        let ops = collect_changes(edit)?;
        if ops.is_empty() {
            return Err(Error::Tool(
                "language server has no rename for this symbol".to_string(),
            ));
        }
        let mut names = Vec::new();
        for op in ops {
            match op {
                Change::Text { uri, edits } => {
                    let dest = self.uri_path(&uri)?;
                    self.policy.write_path(&self.relative(&dest)?)?;
                    let original = std::fs::read_to_string(&dest).map_err(|error| {
                        Error::Tool(format!("cannot read {}: {error}", dest.display()))
                    })?;
                    let edited = apply_text_edits(&original, &edits)?;
                    drop(self.atomic_write(&dest, &self.relative(&dest)?, &edited)?);
                    names.push(self.relative(&dest)?);
                }
                Change::Create { uri } => {
                    let dest = self.uri_path(&uri)?;
                    self.policy.write_path(&self.relative(&dest)?)?;
                    if let Some(parent) = dest.parent() {
                        std::fs::create_dir_all(parent).map_err(|error| {
                            Error::Tool(format!("cannot create {}: {error}", dest.display()))
                        })?;
                    }
                    if !dest.exists() {
                        drop(self.atomic_write(&dest, &self.relative(&dest)?, "")?);
                    }
                    names.push(self.relative(&dest)?);
                }
                Change::Rename { from, to } => {
                    let src = self.uri_path(&from)?;
                    let dest = self.uri_path(&to)?;
                    self.policy.write_path(&self.relative(&src)?)?;
                    self.policy.write_path(&self.relative(&dest)?)?;
                    std::fs::rename(&src, &dest).map_err(|error| {
                        Error::Tool(format!(
                            "cannot rename {} → {}: {error}",
                            src.display(),
                            dest.display()
                        ))
                    })?;
                    names.push(format!(
                        "{} → {}",
                        self.relative(&src)?,
                        self.relative(&dest)?
                    ));
                }
                Change::Delete { uri } => {
                    let dest = self.uri_path(&uri)?;
                    self.policy.write_path(&self.relative(&dest)?)?;
                    std::fs::remove_file(&dest).map_err(|error| {
                        Error::Tool(format!("cannot delete {}: {error}", dest.display()))
                    })?;
                    names.push(self.relative(&dest)?);
                }
            }
        }
        Ok(ToolOutput {
            text: format!("renamed {}", names.join(", ")),
            success: true,
            mutated: true,
            grant: None,
        })
    }

    fn uri_path(&self, uri: &str) -> Result<PathBuf> {
        let path = Url::parse(uri)
            .map_err(|error| Error::Tool(format!("invalid file URI `{uri}`: {error}")))?
            .to_file_path()
            .map_err(|()| Error::Tool(format!("URI `{uri}` is not a local file")))?;
        if path.exists() {
            let canonical = path
                .canonicalize()
                .map_err(|error| Error::Tool(format!("cannot open `{uri}`: {error}")))?;
            self.ensure_inside(&canonical)?;
            return Ok(canonical);
        }
        if let Some(parent) = path.parent().filter(|parent| parent.exists()) {
            let dest = parent
                .canonicalize()
                .map_err(|error| Error::Tool(format!("cannot open `{uri}`: {error}")))?
                .join(path.file_name().unwrap_or_default());
            self.ensure_inside(&dest)?;
            return Ok(dest);
        }
        self.ensure_inside(&path)?;
        Ok(path)
    }

    async fn with_lsp<T>(
        &self,
        path: &Path,
        f: impl for<'a> FnOnce(&'a mut Conn) -> Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>,
    ) -> Result<T> {
        let launch = detect(&self.workspace, path)?;
        let key = launch.key.clone();
        let slot = {
            let mut slots = self
                .lsp
                .lock()
                .map_err(|_poison| Error::Tool("lsp lock poisoned".to_string()))?;
            slots
                .entry(key)
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(None)))
                .clone()
        };
        let mut state = slot.lock().await;
        let mut conn = match state.take() {
            Some(conn) => conn,
            None => tokio::time::timeout(LSP_TIMEOUT, Conn::spawn(&self.workspace, &launch))
                .await
                .map_err(|_elapsed| {
                    Error::Tool("language server initialization timed out".to_string())
                })??,
        };
        match tokio::time::timeout(LSP_TIMEOUT, f(&mut conn)).await {
            Ok(Ok(value)) => {
                *state = Some(conn);
                Ok(value)
            }
            Ok(Err(error)) => Err(error),
            Err(_elapsed) => Err(Error::Tool("language server request timed out".to_string())),
        }
    }
}
