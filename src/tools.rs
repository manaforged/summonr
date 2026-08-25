//! Workspace tools. Reads tag every line; edits address those hashes.

use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::policy::Policy;
use crate::provider::{FunctionDefinition, ToolCall, ToolDefinition, ToolKind, UserInput};
use crate::web::Search;

mod catalog;
mod command;
mod lsp;
pub(crate) mod sandbox;
mod search;
mod workspace;

pub(crate) fn command_exit_code(output: &str) -> Option<i32> {
    command::exit_code(output)
}

/// Events emitted during a turn.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
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
        /// Stable model + system prompt + tool-schema fingerprint.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        prefix_hash: String,
    },
    /// Mail from another mage session, injected into the conversation.
    Mail {
        from: String,
        text: String,
    },
    Aborted {
        reason: String,
    },
    Delta {
        text: String,
    },
    /// A transient provider problem and the recovery underway.
    Note {
        text: String,
    },
    Verify {
        command: String,
        passed: bool,
    },
    /// Harness-owned end-of-turn change and verification summary.
    Receipt {
        text: String,
    },
    Work {
        progress: crate::WorkProgress,
    },
}

/// Workspace-scoped tools gated by [`Policy`].
pub(crate) struct ToolRegistry {
    workspace: PathBuf,
    policy: Policy,
    search: Option<Search>,
    /// Extra environment for every command, on top of the kept host variables.
    env: Vec<(String, String)>,
    sandbox: Option<sandbox::Sandbox>,
    /// Last foreground command and its result, for the repeat short-circuit.
    recent: std::sync::Mutex<Option<Recent>>,
    /// Bumped by every edit or write; a repeat only short-circuits when unchanged.
    writes: std::sync::atomic::AtomicU64,
    /// Signatures of recent calls, for the repeated-call warning.
    history: std::sync::Mutex<std::collections::VecDeque<String>>,
    lsp: std::sync::Mutex<Option<lsp::Conn>>,
}

struct Recent {
    command: String,
    at: std::time::Instant,
    writes: u64,
    output: String,
    success: bool,
}

pub(crate) struct ToolOutput {
    pub(crate) text: String,
    pub(crate) success: bool,
    pub(crate) mutated: bool,
}

impl ToolOutput {
    fn ok(text: String) -> Self {
        Self {
            text,
            success: true,
            mutated: false,
        }
    }
}

impl ToolRegistry {
    /// Environment added to every command this registry runs.
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
            recent: std::sync::Mutex::new(None),
            writes: std::sync::atomic::AtomicU64::new(0),
            history: std::sync::Mutex::new(std::collections::VecDeque::new()),
            lsp: std::sync::Mutex::new(None),
        })
    }

    pub(crate) fn set_strict(&mut self) {
        self.policy.set_strict();
    }

    pub(crate) fn exists(&self, path: &str) -> Result<bool> {
        Ok(self.lexical(path)?.exists())
    }

    pub(crate) fn enable_sandbox(&mut self, deny_network: bool) -> Result<()> {
        self.sandbox = Some(sandbox::Sandbox::discover(&self.workspace, deny_network)?);
        Ok(())
    }

    pub(crate) fn is_strict(&self) -> bool {
        self.policy.strict
    }

    pub(crate) fn is_read_only(&self) -> bool {
        self.policy.is_read_only()
    }

    /// Plan mode: keep the provider schema stable and deny mutation at execution.
    pub(crate) fn set_read_only(&mut self, on: bool) {
        self.policy.set_read_only(on);
    }

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
        if arguments.contains("\"fresh\":true") {
            return 1;
        }
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
            "read_file" => self.read_file(&call.function.arguments).map(ToolOutput::ok),
            "edit_file" => self.edit_file(&call.function.arguments).await,
            "write_file" => self.write_file(&call.function.arguments).await,
            "list_dir" => self.list_dir(&call.function.arguments).map(ToolOutput::ok),
            "grep" => self.grep(&call.function.arguments).map(ToolOutput::ok),
            "skill" => self.skill(&call.function.arguments).map(ToolOutput::ok),
            "run_command" => self.run_command(&call.function.arguments).await,
            "web_search" => self
                .web_search(&call.function.arguments)
                .await
                .map(ToolOutput::ok),
            "web_fetch" => self
                .web_fetch(&call.function.arguments)
                .await
                .map(ToolOutput::ok),
            "diagnostics" => self.diagnostics(&call.function.arguments).await,
            "definition" => self.definition(&call.function.arguments).await,
            name => Err(Error::Tool(format!("unknown tool `{name}`"))),
        }
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
        if dest.exists() {
            return self.resolve_existing(path);
        }
        let mut ancestor = dest.parent().unwrap_or(&self.workspace).to_path_buf();
        while !ancestor.exists() {
            match ancestor.parent() {
                Some(parent) if parent.starts_with(&self.workspace) => {
                    ancestor = parent.to_path_buf();
                }
                _ => break,
            }
        }
        if ancestor.exists() {
            let canonical = ancestor
                .canonicalize()
                .map_err(|error| Error::Tool(format!("cannot create `{path}`: {error}")))?;
            self.ensure_inside(&canonical)?;
        } else if !ancestor.starts_with(&self.workspace) {
            return Err(Error::Tool("path leaves the workspace".to_string()));
        }
        Ok(dest)
    }

    fn lexical(&self, path: &str) -> Result<PathBuf> {
        let relative = Path::new(path);
        if path.trim().is_empty() {
            return Err(Error::Tool("path is empty".to_string()));
        }
        if relative.is_absolute() {
            return Err(Error::Tool(
                "tools accept workspace-relative paths".to_string(),
            ));
        }
        let mut out = self.workspace.clone();
        for component in relative.components() {
            match component {
                Component::Normal(part) => out.push(part),
                Component::CurDir => {}
                Component::ParentDir => {
                    if !out.pop() || !out.starts_with(&self.workspace) {
                        return Err(Error::Tool("path leaves the workspace".to_string()));
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

    fn ensure_inside(&self, path: &Path) -> Result<()> {
        if path.starts_with(&self.workspace) {
            Ok(())
        } else {
            Err(Error::Tool("path leaves the workspace".to_string()))
        }
    }

    fn relative(&self, path: &Path) -> Result<String> {
        path.strip_prefix(&self.workspace)
            .map(|relative| relative.to_string_lossy().into_owned())
            .map_err(|error| Error::Tool(format!("path leaves the workspace: {error}")))
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

/// Resolve `@path` workspace files into one user prompt.
pub fn attach(workspace: &Path, line: &str) -> Result<UserInput> {
    workspace::attach(workspace, line)
}

/// Detect a supported image media type from raw bytes.
pub(crate) fn image_type(bytes: &[u8]) -> Option<&'static str> {
    workspace::image_type(bytes)
}

/// The largest image attachment, in bytes, accepted in one turn.
pub(crate) const IMAGES_MAX_BYTES: usize = workspace::IMAGES_MAX_BYTES;

pub(crate) fn digest(command: &str, raw: &str) -> String {
    command::digest(command, raw)
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
