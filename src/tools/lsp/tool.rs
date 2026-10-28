use std::time::Duration;

use super::*;
use crate::tools::workspace::write;

enum Operation {
    Diagnostics,
    Definition,
}

impl Operation {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "diagnostics" => Some(Self::Diagnostics),
            "definition" => Some(Self::Definition),
            _ => None,
        }
    }
}

impl ToolRegistry {
    pub(in crate::tools) async fn lsp(&self, args: &Value) -> Result<ToolOutput> {
        if let Some(operation) = super::super::args::named(args, "op") {
            return match Operation::parse(operation) {
                Some(Operation::Diagnostics) => self.diagnostics(args).await,
                Some(Operation::Definition) => self.definition(args).await,
                None => Err(Error::Tool(format!("unknown lsp operation `{operation}`"))),
            };
        }
        let placed = |field| {
            args.get(field)
                .and_then(Value::as_u64)
                .is_some_and(|n| n > 0)
        };
        if placed("line")
            || placed("character")
            || args.get("references").and_then(Value::as_bool) == Some(true)
            || args
                .get("new_name")
                .and_then(Value::as_str)
                .is_some_and(|name| !name.trim().is_empty())
        {
            return self.definition(args).await;
        }
        self.diagnostics(args).await
    }

    pub(in crate::tools) async fn diagnostics(&self, args: &Value) -> Result<ToolOutput> {
        let path = nonempty_path(args, "diagnostics")?;
        let file = self.resolve_existing(path)?;
        let request_path = file.clone();
        let (text, _) = self
            .with_lsp(
                &file,
                Scope::Request,
                crate::limits::limits().lsp_request(),
                move |conn| Box::pin(async move { conn.diagnostics(&request_path).await }),
            )
            .await?;
        Ok(ToolOutput::ok(text))
    }

    pub(in crate::tools) async fn definition(&self, args: &Value) -> Result<ToolOutput> {
        let path = nonempty_path(args, "definition")?;
        let file = self.resolve_existing(path)?;
        let line = object_usize(args, "line").unwrap_or(1).max(1) as u32;
        let character = match object_usize(args, "character") {
            Some(character) => character.max(1) as u32,
            None if args.get("line").is_some() => {
                return Err(Error::Tool(format!(
                    "definition at line {line} needs `character`; use the column a read search hit prints (path:line:col)"
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
                    "this session is read-only; no rename".to_string(),
                ));
            }
            let name = new_name.to_string();
            let request_path = file.clone();
            let edit = self
                .with_lsp(
                    &file,
                    Scope::Request,
                    crate::limits::limits().lsp_request(),
                    move |conn| {
                        Box::pin(
                            async move { conn.rename(&request_path, line, character, &name).await },
                        )
                    },
                )
                .await?;
            return self.apply_workspace_edit(&edit);
        }
        let refs = object_bool(args, "references");
        let request_path = file.clone();
        let text = self
            .with_lsp(
                &file,
                Scope::Request,
                crate::limits::limits().lsp_request(),
                move |conn| {
                    Box::pin(async move {
                        if refs {
                            conn.references(&request_path, line, character).await
                        } else {
                            conn.definition(&request_path, line, character).await
                        }
                    })
                },
            )
            .await?;
        Ok(ToolOutput::ok(text))
    }

    pub(in crate::tools) async fn check_after_edit(&self, dest: &Path) -> Option<ToolOutput> {
        if self.lsp.lock().ok()?.is_empty() {
            return None;
        }
        let launch = self
            .launches
            .launch(&self.workspace, dest, Scope::Edit)
            .ok()?;
        let slot = self.lsp.lock().ok()?.get(&launch.key).cloned()?;
        if slot.lock().await.is_none() {
            return None;
        }
        let request_path = dest.to_path_buf();
        let (text, errors) = self
            .with_lsp(
                dest,
                Scope::Edit,
                crate::limits::limits().lsp_check(),
                move |conn| Box::pin(async move { conn.diagnostics(&request_path).await }),
            )
            .await
            .ok()?;
        Some(ToolOutput {
            text: format!("diagnostics: {text}"),
            success: !errors,
            mutated: false,
            grant: None,
            images: Vec::new(),
            patch: None,
        })
    }

    pub(in crate::tools) fn apply_workspace_edit(&self, edit: &Value) -> Result<ToolOutput> {
        let ops = collect_changes(edit)?;
        if ops.is_empty() {
            return Err(Error::Tool(
                "language server has no rename for this symbol".to_string(),
            ));
        }
        let mut changes = Vec::with_capacity(ops.len());
        let mut names = Vec::with_capacity(ops.len());
        for op in ops {
            names.push(self.plan_lsp_change(op, &mut changes)?);
        }
        self.apply_changes(&changes)?;
        let text = format!("renamed {}", names.join(", "));
        Ok(ToolOutput::mutated(text, None))
    }

    fn plan_lsp_change(&self, op: Change, changes: &mut Vec<write::Change>) -> Result<String> {
        let uri = match &op {
            Change::Text { uri, .. } | Change::Create { uri } | Change::Delete { uri } => uri,
            Change::Rename { from, .. } => from,
        };
        let dest = self.uri_path(uri)?;
        self.check_write(&dest)?;
        let name = self.relative(&dest)?;
        let pending = write::staged(changes, &dest);
        let (action, label) = match op {
            Change::Text { edits, .. } => {
                let original = match pending {
                    Some(content) => content.unwrap_or_default().to_string(),
                    None => std::fs::read_to_string(&dest).map_err(|error| {
                        Error::Tool(format!("cannot read {}: {error}", dest.display()))
                    })?,
                };
                let edited = apply_text_edits(&original, &edits)?;
                (write::Action::Write(edited), name.clone())
            }
            Change::Create { .. } if pending.map_or_else(|| dest.exists(), |c| c.is_some()) => {
                return Ok(name);
            }
            Change::Create { .. } => (write::Action::Write(String::new()), name.clone()),
            Change::Delete { .. } => (write::Action::Delete, name.clone()),
            Change::Rename { to, .. } => {
                let to = self.uri_path(&to)?;
                self.check_write(&to)?;
                let display = self.relative(&to)?;
                let label = format!("{name} → {display}");
                (write::Action::Move { to, display }, label)
            }
        };
        changes.push(write::Change {
            dest,
            display: name,
            action,
        });
        Ok(label)
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
        scope: Scope,
        deadline: Duration,
        f: impl for<'a> FnOnce(&'a mut Conn) -> Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>,
    ) -> Result<T> {
        let launch = self.launches.launch(&self.workspace, path, scope)?;
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
            None => {
                let conn = tokio::time::timeout(
                    crate::limits::limits().lsp_start(),
                    Conn::spawn(&self.workspace, &launch),
                )
                .await
                .map_err(|_elapsed| {
                    Error::Tool(format!(
                        "{} did not start within {} s",
                        launch.argv[0],
                        crate::limits::limits().lsp_start_secs
                    ))
                })??;
                crate::supervise::register(
                    crate::supervise::ServerKind::Lsp,
                    &launch.key,
                    conn.pid(),
                    Some(crate::limits::limits().lsp_idle()),
                    &slot,
                );
                conn
            }
        };
        conn.set_wait(deadline);
        let outcome = f(&mut conn).await;
        crate::supervise::touch(&slot);
        if conn.alive() {
            *state = Some(conn);
        }
        outcome
    }
}
