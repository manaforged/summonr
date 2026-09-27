use std::time::Duration;

use super::detect::detect;
use super::*;
use crate::limits::{LSP_CHECK, LSP_REQUEST, LSP_START};

impl ToolRegistry {
    pub(in crate::tools) async fn lsp(&self, arguments: &str) -> Result<ToolOutput> {
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

    pub(in crate::tools) async fn diagnostics(&self, arguments: &str) -> Result<ToolOutput> {
        let args = parse_args("diagnostics", arguments)?;
        let path = nonempty_path(&args, "diagnostics")?;
        let file = self.resolve_existing(path)?;
        let request_path = file.clone();
        let text = self
            .with_lsp(&file, LSP_REQUEST, move |conn| {
                Box::pin(async move { conn.diagnostics(&request_path).await })
            })
            .await?;
        Ok(ToolOutput::ok(text))
    }

    pub(in crate::tools) async fn definition(&self, arguments: &str) -> Result<ToolOutput> {
        let args = parse_args("definition", arguments)?;
        let path = nonempty_path(&args, "definition")?;
        let file = self.resolve_existing(path)?;
        let line = object_usize(&args, "line").unwrap_or(1).max(1) as u32;
        let character = match object_usize(&args, "character") {
            Some(character) => character.max(1) as u32,
            None if args.get("line").is_some() => {
                return Err(Error::Tool(format!(
                    "definition at line {line} needs `character` — use the column a read search hit prints (path:line:col)"
                )));
            }
            None => 1,
        };
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
                .with_lsp(&file, LSP_REQUEST, move |conn| {
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
            .with_lsp(&file, LSP_REQUEST, move |conn| {
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

    pub(in crate::tools) async fn check_after_edit(&self, dest: &Path) -> Option<ToolOutput> {
        let key = detect(&self.workspace, dest).ok()?.key;
        let slot = self.lsp.lock().ok()?.get(&key).cloned()?;
        if slot.lock().await.is_none() {
            return None;
        }
        let request_path = dest.to_path_buf();
        let text = self
            .with_lsp(dest, LSP_CHECK, move |conn| {
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

    pub(in crate::tools) fn apply_workspace_edit(&self, edit: &Value) -> Result<ToolOutput> {
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
                    self.check_write(&dest)?;
                    let original = std::fs::read_to_string(&dest).map_err(|error| {
                        Error::Tool(format!("cannot read {}: {error}", dest.display()))
                    })?;
                    let edited = apply_text_edits(&original, &edits)?;
                    drop(self.atomic_write(&dest, &self.relative(&dest)?, &edited, false)?);
                    names.push(self.relative(&dest)?);
                }
                Change::Create { uri } => {
                    let dest = self.uri_path(&uri)?;
                    self.check_write(&dest)?;
                    if !dest.exists() {
                        drop(self.atomic_write(&dest, &self.relative(&dest)?, "", false)?);
                    }
                    names.push(self.relative(&dest)?);
                }
                Change::Rename { from, to } => {
                    let src = self.uri_path(&from)?;
                    let dest = self.uri_path(&to)?;
                    self.check_write(&src)?;
                    self.check_write(&dest)?;
                    crate::durable::store::rename(&src, &dest).map_err(|error| {
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
                    self.check_write(&dest)?;
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
        deadline: Duration,
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
            None => tokio::time::timeout(LSP_START, Conn::spawn(&self.workspace, &launch))
                .await
                .map_err(|_elapsed| {
                    Error::Tool(format!(
                        "{} did not start within {} s",
                        launch.argv[0],
                        LSP_START.as_secs()
                    ))
                })??,
        };
        let outcome = tokio::time::timeout(deadline, f(&mut conn)).await;
        if conn.clean {
            *state = Some(conn);
        }
        outcome.map_err(|_elapsed| {
            Error::Tool(format!(
                "{} did not answer within {} s; the server may still be indexing",
                launch.argv[0],
                deadline.as_secs()
            ))
        })?
    }
}
