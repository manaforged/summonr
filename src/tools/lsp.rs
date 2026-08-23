//! Language Server Protocol client. One connection: diagnostics, definition, references, rename.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use serde_json::{Value, json};

use crate::error::{Error, Result};

use super::{
    ToolOutput, ToolRegistry, nonempty_path, object_bool, object_usize, parse_args, workspace,
};

pub(super) struct Conn {
    child: Option<Child>,
    reader: BufReader<Box<dyn Read + Send>>,
    writer: Box<dyn Write + Send>,
    next_id: i64,
    root: String,
}

impl Conn {
    pub(super) fn spawn(workspace: &Path) -> Result<Self> {
        let argv = detect(workspace)?;
        let mut child = Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| Error::Tool(format!("cannot start {}: {error}", argv[0])))?;
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
            root: file_uri(workspace),
        };
        conn.handshake()?;
        Ok(conn)
    }

    #[cfg(test)]
    pub(super) fn from_io(
        reader: impl Read + Send + 'static,
        writer: impl Write + Send + 'static,
        root: &Path,
    ) -> Result<Self> {
        let mut conn = Self {
            child: None,
            reader: BufReader::new(Box::new(reader)),
            writer: Box::new(writer),
            next_id: 1,
            root: file_uri(root),
        };
        conn.handshake()?;
        Ok(conn)
    }

    fn handshake(&mut self) -> Result<()> {
        let result = self.request(
            "initialize",
            json!({
                "processId": null,
                "rootUri": self.root,
                "workspaceFolders": [{ "uri": self.root, "name": "workspace" }],
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
        )?;
        drop(result);
        self.notify("initialized", json!({}))?;
        Ok(())
    }

    pub(super) fn diagnostics(&mut self, path: &Path) -> Result<String> {
        self.did_open(path)?;
        let uri = file_uri(path);
        let result = self.request(
            "textDocument/diagnostic",
            json!({ "textDocument": { "uri": uri } }),
        )?;
        let items = diagnostic_items(&result);
        if items.is_empty() {
            return Ok("no diagnostics".to_string());
        }
        Ok(items.join("\n"))
    }

    pub(super) fn definition(&mut self, path: &Path, line: u32, character: u32) -> Result<String> {
        let result = self.at(path, line, character, "textDocument/definition")?;
        let locations = locations(&result);
        if locations.is_empty() {
            return Ok("no definition".to_string());
        }
        Ok(locations.join("\n"))
    }

    pub(super) fn references(&mut self, path: &Path, line: u32, character: u32) -> Result<String> {
        self.did_open(path)?;
        let uri = file_uri(path);
        let line = line.saturating_sub(1);
        let character = character.saturating_sub(1);
        let result = self.request(
            "textDocument/references",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character },
                "context": { "includeDeclaration": true }
            }),
        )?;
        let locations = locations(&result);
        if locations.is_empty() {
            return Ok("no references".to_string());
        }
        Ok(locations.join("\n"))
    }

    pub(super) fn rename(
        &mut self,
        path: &Path,
        line: u32,
        character: u32,
        new_name: &str,
    ) -> Result<Value> {
        self.did_open(path)?;
        let uri = file_uri(path);
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
    }

    fn at(&mut self, path: &Path, line: u32, character: u32, method: &str) -> Result<Value> {
        self.did_open(path)?;
        let uri = file_uri(path);
        let line = line.saturating_sub(1);
        let character = character.saturating_sub(1);
        self.request(
            method,
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": line, "character": character }
            }),
        )
    }

    fn did_open(&mut self, path: &Path) -> Result<()> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| Error::Tool(format!("cannot read {}: {error}", path.display())))?;
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": file_uri(path),
                    "languageId": language_id(path),
                    "version": 1,
                    "text": text
                }
            }),
        )
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        write_msg(
            &mut self.writer,
            &json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params
            }),
        )?;
        self.pump(id)
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        write_msg(
            &mut self.writer,
            &json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params
            }),
        )
    }

    fn pump(&mut self, want: i64) -> Result<Value> {
        loop {
            let msg = read_msg(&mut self.reader)?;
            if msg.get("method").is_some() && msg.get("id").is_some() {
                self.reply_server(&msg)?;
                continue;
            }
            if msg.get("id") == Some(&Value::from(want)) {
                if let Some(error) = msg.get("error") {
                    return Err(Error::Tool(format!("lsp: {error}")));
                }
                return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }

    fn reply_server(&mut self, msg: &Value) -> Result<()> {
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
        write_msg(
            &mut self.writer,
            &json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        )
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        drop(self.notify("exit", json!({})));
        if let Some(mut child) = self.child.take() {
            drop(child.kill());
        }
    }
}

pub(super) fn detect(workspace: &Path) -> Result<Vec<String>> {
    if let Some(command) = configured(workspace) {
        return argv(&command);
    }
    from_tree(workspace)
}

struct Kind {
    markers: &'static [&'static str],
    bins: &'static [(&'static str, &'static [&'static str])],
}

const KINDS: &[Kind] = &[
    Kind {
        markers: &["Cargo.toml"],
        bins: &[("rust-analyzer", &[])],
    },
    Kind {
        markers: &["go.mod"],
        bins: &[("gopls", &[])],
    },
    Kind {
        markers: &["package.json"],
        bins: &[
            ("biome", &["lsp-proxy"]),
            ("typescript-language-server", &["--stdio"]),
        ],
    },
    Kind {
        markers: &["pyproject.toml", "setup.py"],
        bins: &[("ruff", &["server"]), ("pyright-langserver", &["--stdio"])],
    },
];

fn from_tree(workspace: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    for kind in KINDS {
        if !kind
            .markers
            .iter()
            .any(|marker| workspace.join(marker).is_file())
        {
            continue;
        }
        for (name, args) in kind.bins {
            names.push(*name);
            if on_path(name) {
                let mut command = vec![(*name).to_string()];
                command.extend(args.iter().map(|arg| (*arg).to_string()));
                return Ok(command);
            }
        }
        return Err(Error::Tool(format!(
            "{} not on PATH; set [lsp] command in .mage/config.toml",
            names.join(" or ")
        )));
    }
    Err(Error::Tool(
        "no language server: set [lsp] command in .mage/config.toml".to_string(),
    ))
}

fn configured(workspace: &Path) -> Option<Vec<String>> {
    read_command(&workspace.join(".mage/config.toml"))
        .or_else(|| crate::config::load().ok().and_then(|file| file.lsp.command))
        .filter(|command| !command.is_empty())
}

fn read_command(path: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(path).ok()?;
    let file: crate::config::File = toml::from_str(&text).ok()?;
    file.lsp.command
}

fn argv(command: &[String]) -> Result<Vec<String>> {
    let name = command
        .first()
        .map(String::as_str)
        .filter(|name| !name.is_empty())
        .ok_or_else(|| Error::Tool("[lsp] command is empty".to_string()))?;
    if !on_path(name) {
        return Err(Error::Tool(format!(
            "{name} is not on PATH; set [lsp] command to your language server"
        )));
    }
    Ok(command.to_vec())
}

fn on_path(name: &str) -> bool {
    let Ok(path) = std::env::var("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(name).is_file())
}

fn file_uri(path: &Path) -> String {
    let raw = path.to_string_lossy();
    format!("file://{}", raw.replace(' ', "%20"))
}

fn language_id(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("rs") => "rust",
        Some("go") => "go",
        Some("ts") => "typescript",
        Some("tsx") => "typescriptreact",
        Some("js" | "mjs" | "cjs") => "javascript",
        Some("jsx") => "javascriptreact",
        Some("py") => "python",
        Some("toml") => "toml",
        _ => "plaintext",
    }
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

/// LSP `character` is UTF-16 code units on the line.
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

pub(super) fn write_msg(writer: &mut impl Write, value: &Value) -> Result<()> {
    let body = serde_json::to_vec(value)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())
        .map_err(|error| Error::Tool(format!("lsp write: {error}")))?;
    writer
        .write_all(&body)
        .map_err(|error| Error::Tool(format!("lsp write: {error}")))?;
    writer
        .flush()
        .map_err(|error| Error::Tool(format!("lsp write: {error}")))?;
    Ok(())
}

pub(super) fn read_msg(reader: &mut BufReader<impl Read>) -> Result<Value> {
    let mut length = None;
    loop {
        let mut line = String::new();
        let n = reader
            .read_line(&mut line)
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
        .map_err(|error| Error::Tool(format!("lsp read: {error}")))?;
    serde_json::from_slice(&body).map_err(|error| Error::Tool(format!("lsp json: {error}")))
}

impl ToolRegistry {
    pub(super) async fn diagnostics(&self, arguments: &str) -> Result<ToolOutput> {
        let args = parse_args("diagnostics", arguments)?;
        let path = nonempty_path(&args, "diagnostics")?;
        let file = self.resolve_existing(path)?;
        let text = tokio::task::block_in_place(|| self.with_lsp(|conn| conn.diagnostics(&file)))?;
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
            let edit = tokio::task::block_in_place(|| {
                self.with_lsp(|conn| conn.rename(&file, line, character, &name))
            })?;
            return self.apply_workspace_edit(&edit);
        }
        let refs = object_bool(&args, "references");
        let text = tokio::task::block_in_place(|| {
            self.with_lsp(|conn| {
                if refs {
                    conn.references(&file, line, character)
                } else {
                    conn.definition(&file, line, character)
                }
            })
        })?;
        Ok(ToolOutput::ok(text))
    }

    pub(super) async fn check_after_edit(&self, dest: &Path) -> Option<ToolOutput> {
        let live = self.lsp.lock().ok()?.is_some();
        if !live {
            return None;
        }
        let text = self.with_lsp(|conn| conn.diagnostics(dest)).ok()?;
        Some(ToolOutput {
            text: format!("diagnostics: {text}"),
            success: !text.starts_with("error "),
            mutated: false,
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
                    workspace::atomic_write(&dest, &self.relative(&dest)?, &edited)?;
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
                        workspace::atomic_write(&dest, &self.relative(&dest)?, "")?;
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
            self.writes
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(ToolOutput {
            text: format!("renamed {}", names.join(", ")),
            success: true,
            mutated: true,
        })
    }

    fn uri_path(&self, uri: &str) -> Result<PathBuf> {
        let raw = uri
            .strip_prefix("file://")
            .unwrap_or(uri)
            .replace("%20", " ");
        let path = PathBuf::from(raw);
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

    fn with_lsp<T>(&self, f: impl FnOnce(&mut Conn) -> Result<T>) -> Result<T> {
        let mut slot = self
            .lsp
            .lock()
            .map_err(|_poison| Error::Tool("lsp lock poisoned".to_string()))?;
        let conn = ensure(&mut slot, &self.workspace)?;
        f(conn)
    }
}

pub(super) fn ensure<'a>(slot: &'a mut Option<Conn>, workspace: &Path) -> Result<&'a mut Conn> {
    if slot.is_none() {
        *slot = Some(Conn::spawn(workspace)?);
    }
    slot.as_mut()
        .ok_or_else(|| Error::Tool("language server is missing".to_string()))
}

#[cfg(test)]
#[path = "lsp_tests.rs"]
mod tests;
