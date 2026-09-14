use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::policy::{Grant, Policy};
use crate::provider::{FunctionDefinition, ToolCall, ToolDefinition, ToolKind, UserInput};
use crate::web::Search;

mod catalog;
pub(crate) use catalog::readonly;
mod command;
mod lsp;
pub(crate) mod sandbox;
mod search;
mod workspace;

pub(crate) fn command_exit_code(output: &str) -> Option<i32> {
    command::exit_code(output)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
    ToolResult {
        id: String,
        success: bool,
        #[serde(default)]
        mutated: bool,
        output: String,
    },
    Assistant {
        text: String,
    },
    Usage {
        prompt_tokens: u32,
        completion_tokens: u32,
        #[serde(default)]
        reasoning_tokens: u32,
        cached_tokens: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        millis: Option<u64>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        prefix_hash: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cost: Option<f64>,
    },
    Unreported {
        tokens: u64,
    },
    Mail {
        from: String,
        text: String,
    },
    Aborted {
        reason: String,
    },
    Merged {
        branch: String,
        base: String,
        files: Vec<String>,
    },
    Delta {
        text: String,
    },
    Reasoning {
        text: String,
    },
    Note {
        text: String,
    },
    Approval {
        detail: Option<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        persist: bool,
    },
    Verify {
        command: String,
        passed: bool,
    },
    Receipt {
        receipt: crate::Receipt,
    },
    Work {
        progress: crate::WorkProgress,
    },
    Child {
        id: String,
        event: Box<Event>,
    },
}

pub(crate) struct ToolRegistry {
    workspace: PathBuf,
    policy: Policy,
    search: Option<Search>,
    env: Vec<(String, String)>,
    sandbox: Option<sandbox::Sandbox>,
    background: bool,
    language: bool,
    skills: bool,
    history: std::sync::Mutex<std::collections::VecDeque<String>>,
    lsp: std::sync::Mutex<
        std::collections::BTreeMap<String, std::sync::Arc<tokio::sync::Mutex<Option<lsp::Conn>>>>,
    >,
    session: std::sync::Mutex<Option<String>>,
    network_once: std::sync::atomic::AtomicBool,
    approved_once: std::sync::atomic::AtomicBool,
}

pub(crate) struct ToolOutput {
    pub(crate) text: String,
    pub(crate) success: bool,
    pub(crate) mutated: bool,
    pub(crate) grant: Option<Grant>,
}

impl ToolOutput {
    fn ok(text: String) -> Self {
        Self {
            text,
            success: true,
            mutated: false,
            grant: None,
        }
    }
}

pub(crate) fn needed_grant(result: &Result<ToolOutput>) -> Option<Grant> {
    match result {
        Ok(output) => output.grant.clone(),
        Err(error) => crate::policy::grant_from_error(error),
    }
}

impl ToolRegistry {
    pub(crate) fn set_env(&mut self, env: impl IntoIterator<Item = (String, String)>) {
        self.env = env.into_iter().collect();
    }

    pub(crate) fn with_policy_and_search(
        workspace: impl AsRef<Path>,
        policy: Policy,
        search: Option<Search>,
    ) -> Result<Self> {
        let workspace = workspace
            .as_ref()
            .canonicalize()
            .map_err(|error| Error::Tool(format!("cannot open workspace: {error}")))?;
        if !workspace.is_dir() {
            return Err(Error::Tool("workspace is not a directory".to_string()));
        }
        Ok(Self {
            workspace,
            policy,
            search,
            env: Vec::new(),
            sandbox: None,
            background: true,
            language: true,
            skills: true,
            history: std::sync::Mutex::new(std::collections::VecDeque::new()),
            lsp: std::sync::Mutex::new(std::collections::BTreeMap::new()),
            session: std::sync::Mutex::new(None),
            network_once: std::sync::atomic::AtomicBool::new(false),
            approved_once: std::sync::atomic::AtomicBool::new(false),
        })
    }

    pub(crate) fn set_modules(&mut self, search: bool, lsp: bool, jobs: bool, skills: bool) {
        if !search {
            self.search = None;
        }
        self.language = lsp;
        self.background = jobs;
        self.skills = skills;
    }

    pub(crate) fn set_strict(&mut self) {
        self.policy.set_strict();
    }

    pub(crate) fn set_unrestricted(&mut self) {
        self.policy.set_unrestricted();
    }

    pub(crate) fn child_at(&self, workspace: &Path, session: &str) -> Result<Self> {
        let mut child = Self::with_policy_and_search(workspace, self.policy.clone(), None)?;
        child.env.clone_from(&self.env);
        child.sandbox = self
            .sandbox
            .as_ref()
            .map(|sandbox| sandbox.at(&child.workspace))
            .transpose()?;
        child.background = false;
        child.language = self.language;
        child.skills = self.skills;
        child.set_session(session);
        Ok(child)
    }

    pub(crate) fn exists(&self, path: &str) -> Result<bool> {
        Ok(self.lexical(path)?.exists())
    }

    pub(crate) fn enable_sandbox(&mut self, deny_network: bool) -> Result<()> {
        self.sandbox = Some(sandbox::Sandbox::discover(
            &self.workspace,
            deny_network,
            &self.policy.write_roots(),
        )?);
        Ok(())
    }

    pub(crate) fn allow_path(&mut self, path: PathBuf) -> Result<()> {
        if crate::policy::protected_write(&path) {
            return Err(Error::Denied(format!(
                "`{}` is a protected path",
                path.display()
            )));
        }
        if !path.exists() {
            std::fs::create_dir_all(&path).map_err(|error| {
                Error::Config(format!(
                    "cannot create write path {}: {error}",
                    path.display()
                ))
            })?;
        }
        let resolved = path.canonicalize().map_err(|error| {
            Error::Config(format!(
                "write path {} is unusable: {error}",
                path.display()
            ))
        })?;
        if crate::policy::protected_write(&resolved) {
            return Err(Error::Denied(format!(
                "`{}` is a protected path",
                resolved.display()
            )));
        }
        if !resolved.is_dir() {
            return Err(Error::Config(format!(
                "write path {} is not a directory",
                resolved.display()
            )));
        }
        self.policy.allow_path(resolved);
        if self.sandbox.is_some() {
            let deny = self
                .sandbox
                .as_ref()
                .is_some_and(sandbox::Sandbox::denies_network);
            self.enable_sandbox(deny)?;
        }
        Ok(())
    }

    pub(crate) fn write_roots(&self) -> Vec<String> {
        self.policy
            .write_roots()
            .iter()
            .map(|path| path.display().to_string())
            .collect()
    }

    pub(crate) fn allow_network_once(&self) {
        self.network_once
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn approve_once(&self) {
        self.approved_once
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn take_approval(&self) -> bool {
        self.approved_once
            .swap(false, std::sync::atomic::Ordering::SeqCst)
    }

    pub(crate) fn has_sandbox(&self) -> bool {
        self.sandbox.is_some()
    }

    pub(crate) fn denies_network(&self) -> bool {
        self.sandbox
            .as_ref()
            .is_some_and(sandbox::Sandbox::denies_network)
    }

    pub(crate) fn is_strict(&self) -> bool {
        self.policy.strict
    }

    pub(crate) fn is_read_only(&self) -> bool {
        self.policy.is_read_only()
    }

    pub(crate) fn set_session(&self, id: &str) {
        if let Ok(mut session) = self.session.lock() {
            *session = Some(id.to_string());
        }
    }

    pub(crate) fn set_read_only(&mut self, on: bool) {
        self.policy.set_read_only(on);
    }

    #[cfg(test)]
    pub(crate) async fn execute(&self, call: &ToolCall) -> Result<String> {
        self.execute_outcome(call).await.map(|output| output.text)
    }

    pub(crate) async fn execute_outcome(&self, call: &ToolCall) -> Result<ToolOutput> {
        if !self.enabled(&call.function.name) {
            return if catalog::known(&call.function.name) {
                Err(Error::Denied(format!(
                    "tool `{}` is unavailable in the current mode",
                    call.function.name
                )))
            } else {
                Err(Error::Tool(format!(
                    "unknown tool `{}`",
                    call.function.name
                )))
            };
        }
        let repeats = self.note_call(&call.function.name, &call.function.arguments);
        let result = self.dispatch(call).await;
        if repeats < 3 {
            return result;
        }
        let note = format!(
            "[{repeats} identical calls in a row — the result will not change; re-read the step, \
             change the approach, or ask]"
        );
        match result {
            Ok(mut output) => {
                output.text = format!("{note}\n{}", output.text);
                Ok(output)
            }
            Err(Error::Tool(text)) => Err(Error::Tool(format!("{note}\n{text}"))),
            Err(other) => Err(other),
        }
    }

    fn note_call(&self, name: &str, arguments: &str) -> usize {
        let signature = format!("{name}\u{0}{arguments}");
        let Ok(mut history) = self.history.lock() else {
            return 1;
        };
        history.push_back(signature.clone());
        if history.len() > 8 {
            history.pop_front();
        }
        history
            .iter()
            .rev()
            .take_while(|past| **past == signature)
            .count()
    }

    async fn dispatch(&self, call: &ToolCall) -> Result<ToolOutput> {
        match call.function.name.as_str() {
            "read" => self.read(&call.function.arguments).map(ToolOutput::ok),
            "read_file" => self.read_file(&call.function.arguments).map(ToolOutput::ok),
            "edit" | "edit_file" => self.edit_file(&call.function.arguments).await,
            name @ ("replace_text" | "replace_texts" | "replace_lines" | "insert_after"
            | "delete_lines") => self.edit_operation(name, &call.function.arguments).await,
            "write" | "write_file" => self.write_file(&call.function.arguments).await,
            "list_dir" => self.list_dir(&call.function.arguments).map(ToolOutput::ok),
            "grep" => self.grep(&call.function.arguments).map(ToolOutput::ok),
            "show_image" => self
                .show_image(&call.function.arguments)
                .map(ToolOutput::ok),
            "skill" => self.skill(&call.function.arguments).map(ToolOutput::ok),
            "run_command" => self.run_command(&call.function.arguments).await,
            "web" => self.web(&call.function.arguments).await.map(ToolOutput::ok),
            "web_search" => self
                .web_search(&call.function.arguments)
                .await
                .map(ToolOutput::ok),
            "web_fetch" => self
                .web_fetch(&call.function.arguments)
                .await
                .map(ToolOutput::ok),
            "lsp" => self.lsp(&call.function.arguments).await,
            "diagnostics" => self.diagnostics(&call.function.arguments).await,
            "definition" => self.definition(&call.function.arguments).await,
            name => Err(Error::Tool(format!("unknown tool `{name}`"))),
        }
    }

    fn resolve_read(&self, path: &str) -> Result<PathBuf> {
        let candidate = Path::new(path);
        let opened = if candidate.is_absolute() {
            candidate
                .canonicalize()
                .map_err(|error| Error::Tool(format!("cannot open `{path}`: {error}")))?
        } else {
            self.resolve_existing(path)?
        };
        if !self.policy.is_unrestricted() && crate::policy::protected_read(&opened) {
            return Err(Error::Denied(format!("`{path}` is a protected path")));
        }
        Ok(opened)
    }

    fn resolve_existing(&self, path: &str) -> Result<PathBuf> {
        let candidate = self.lexical(path)?;
        let canonical = candidate
            .canonicalize()
            .map_err(|error| Error::Tool(format!("cannot open `{path}`: {error}")))?;
        self.ensure_inside(&canonical)?;
        Ok(canonical)
    }

    fn resolve_write(&self, path: &str) -> Result<PathBuf> {
        let dest = self.lexical(path)?;
        if self.policy.is_unrestricted() {
            return Ok(dest);
        }
        if dest.exists() {
            return self.resolve_existing(path);
        }
        let mut ancestor = dest.parent().unwrap_or(&self.workspace).to_path_buf();
        while !ancestor.exists() {
            match ancestor.parent() {
                Some(parent) => ancestor = parent.to_path_buf(),
                None => break,
            }
        }
        if ancestor.exists() {
            let canonical = ancestor
                .canonicalize()
                .map_err(|error| Error::Tool(format!("cannot create `{path}`: {error}")))?;
            self.ensure_inside(&canonical)?;
        } else if !self.inside_fence(&ancestor) {
            return Err(Error::Tool(crate::policy::write_denied(
                &ancestor.display().to_string(),
            )));
        }
        Ok(dest)
    }

    fn lexical(&self, path: &str) -> Result<PathBuf> {
        let relative = Path::new(path);
        if path.trim().is_empty() {
            return Err(Error::Tool("path is empty".to_string()));
        }
        if self.policy.is_unrestricted() {
            return Ok(if relative.is_absolute() {
                relative.to_path_buf()
            } else {
                self.workspace.join(relative)
            });
        }
        if relative.is_absolute() {
            self.ensure_inside(relative)?;
            return Ok(relative.to_path_buf());
        }
        let mut out = self.workspace.clone();
        for component in relative.components() {
            match component {
                Component::Normal(part) => out.push(part),
                Component::CurDir => {}
                Component::ParentDir => {
                    if !out.pop() {
                        return Err(Error::Tool(crate::policy::write_denied(
                            &out.display().to_string(),
                        )));
                    }
                }
                Component::Prefix(_) | Component::RootDir => {
                    return Err(Error::Tool(
                        "tools accept workspace-relative paths".to_string(),
                    ));
                }
            }
        }
        self.ensure_inside(&out)?;
        Ok(out)
    }

    fn inside_fence(&self, path: &Path) -> bool {
        self.policy.is_unrestricted() || path.starts_with(&self.workspace) || self.approved(path)
    }

    fn approved(&self, path: &Path) -> bool {
        let roots = self.policy.write_roots();
        if roots.is_empty() {
            return false;
        }
        let resolved = path.canonicalize().unwrap_or_else(|_| {
            path.parent()
                .and_then(|parent| parent.canonicalize().ok())
                .map_or_else(
                    || path.to_path_buf(),
                    |parent| parent.join(path.file_name().unwrap_or_default()),
                )
        });
        roots.iter().any(|root| {
            let root = root.canonicalize().unwrap_or_else(|_| root.clone());
            resolved.starts_with(&root) || path.starts_with(&root)
        })
    }

    fn check_write(&self, path: &Path) -> Result<()> {
        if self.policy.is_unrestricted() {
            return Ok(());
        }
        if crate::policy::protected_write(path) {
            return Err(Error::Denied(format!(
                "`{}` is a protected path",
                path.display()
            )));
        }
        if self.approved(path) {
            return Ok(());
        }
        self.policy.write_path(&self.relative(path)?)
    }

    fn ensure_inside(&self, path: &Path) -> Result<()> {
        if self.inside_fence(path) {
            Ok(())
        } else {
            Err(Error::Tool(crate::policy::write_denied(
                &path.display().to_string(),
            )))
        }
    }

    fn relative(&self, path: &Path) -> Result<String> {
        if self.policy.is_unrestricted() {
            return Ok(path.to_string_lossy().into_owned());
        }
        path.strip_prefix(&self.workspace)
            .map(|relative| relative.to_string_lossy().into_owned())
            .map_err(|_| Error::Tool(crate::policy::write_denied(&path.display().to_string())))
    }
}

pub(crate) fn tool(name: &str, description: &str, parameters: Value) -> ToolDefinition {
    ToolDefinition {
        kind: ToolKind::Function,
        function: FunctionDefinition {
            name: name.to_string(),
            description: description.to_string(),
            parameters,
        },
    }
}

fn parse_args(name: &str, arguments: &str) -> Result<Value> {
    serde_json::from_str(arguments)
        .map_err(|error| Error::Tool(format!("{name} arguments are invalid: {error}")))
}

fn object_string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Tool(format!("{field} is required")))
}

fn nonempty_path<'a>(args: &'a Value, tool_name: &str) -> Result<&'a str> {
    args.get("path")
        .and_then(Value::as_str)
        .filter(|path| !path.trim().is_empty())
        .ok_or_else(|| Error::Tool(format!("{tool_name} requires a non-empty path")))
}

fn object_usize(value: &Value, field: &str) -> Option<usize> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
}

fn object_bool(value: &Value, field: &str) -> bool {
    value.get(field).and_then(Value::as_bool).unwrap_or(false)
}

pub fn attach(workspace: &Path, line: &str) -> Result<UserInput> {
    workspace::attach(workspace, line)
}

pub(crate) fn image_type(bytes: &[u8]) -> Option<&'static str> {
    workspace::image_type(bytes)
}

pub(crate) const IMAGES_MAX_BYTES: usize = workspace::IMAGES_MAX_BYTES;

pub(crate) fn digest(command: &str, raw: &str) -> String {
    command::digest(command, raw)
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
