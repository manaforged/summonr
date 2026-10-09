use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

mod config;

const OUTPUT_MAX: u64 = 64 * 1024;
const CLAUDE_NAMES: [(&str, &str); 9] = [
    ("run_command", "Bash"),
    ("read", "Read"),
    ("search", "Grep"),
    ("edit", "Edit"),
    ("write", "Write"),
    ("todo", "TodoWrite"),
    ("task", "Task"),
    ("web", "WebFetch"),
    ("skill", "Skill"),
];
const CLAUDE_ALIASES: [(&str, &str); 5] = [
    ("search", "Glob"),
    ("edit", "MultiEdit"),
    ("edit", "NotebookEdit"),
    ("web", "WebSearch"),
    ("lsp", "LSP"),
];

#[must_use]
pub(crate) fn builtin(name: &str) -> Option<crate::tools::Builtin> {
    let name = name.trim();
    let native = CLAUDE_NAMES
        .iter()
        .chain(&CLAUDE_ALIASES)
        .find(|(_, claude)| claude.eq_ignore_ascii_case(name))
        .map_or(name, |(native, _)| *native);
    crate::tools::Builtin::live(native)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HookEvent {
    PreTool,
    PostTool,
    Compact,
    Stop,
}

impl HookEvent {
    pub(crate) const ALL: [Self; 4] = [Self::PreTool, Self::PostTool, Self::Compact, Self::Stop];

    #[must_use]
    pub(crate) const fn key(self) -> &'static str {
        match self {
            Self::PreTool => "pre_tool",
            Self::PostTool => "post_tool",
            Self::Compact => "compact",
            Self::Stop => "stop",
        }
    }

    #[must_use]
    pub(crate) const fn claude(self) -> &'static str {
        match self {
            Self::PreTool => "PreToolUse",
            Self::PostTool => "PostToolUse",
            Self::Compact => "PreCompact",
            Self::Stop => "Stop",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Dialect {
    #[default]
    Summonr,
    Claude,
}

#[derive(Clone, Debug)]
pub(crate) enum Matcher {
    Names(Vec<String>),
    Pattern(regex::Regex),
}

impl Matcher {
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() || text == "*" {
            return None;
        }
        let plain = text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' ' | ',' | '|'));
        if plain {
            return Some(Self::Names(
                text.split(['|', ','])
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_string)
                    .collect(),
            ));
        }
        regex::Regex::new(text).ok().map(Self::Pattern)
    }

    fn matches(&self, tool: &str) -> bool {
        match self {
            Self::Names(names) => names.iter().any(|name| name == tool),
            Self::Pattern(pattern) => pattern.is_match(tool),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Hook {
    pub(crate) command: String,
    pub(crate) dialect: Dialect,
    pub(crate) matcher: Option<Matcher>,
    pub(crate) root: Option<PathBuf>,
}

impl Hook {
    fn plain(command: String, dialect: Dialect) -> Self {
        Self {
            command,
            dialect,
            matcher: None,
            root: None,
        }
    }

    pub(crate) fn applies(&self, tool: Option<&str>) -> bool {
        let (Some(matcher), Some(tool)) = (&self.matcher, tool) else {
            return true;
        };
        matcher.matches(&self.tool_name(tool))
            || (self.dialect != Dialect::Summonr
                && CLAUDE_ALIASES
                    .iter()
                    .any(|(ours, theirs)| *ours == tool && matcher.matches(theirs)))
    }

    pub(crate) fn tool_name(&self, tool: &str) -> String {
        if self.dialect == Dialect::Summonr {
            return tool.to_string();
        }
        CLAUDE_NAMES
            .iter()
            .find(|(ours, _)| *ours == tool)
            .map_or_else(|| mcp_name(tool), |(_, theirs)| (*theirs).to_string())
    }

    fn claude_payload(&self, call: &Call<'_>) -> Value {
        let payload = call.payload;
        let mut body = json!({
            "session_id": call.session,
            "cwd": call.workspace,
            "hook_event_name": call.event.claude(),
        });
        if let Some(tool) = payload.get("tool").and_then(Value::as_str) {
            body["tool_name"] = json!(self.tool_name(tool));
            body["tool_input"] = payload.get("arguments").cloned().unwrap_or(Value::Null);
            body["tool_use_id"] = payload.get("id").cloned().unwrap_or(Value::Null);
        }
        if let Some(output) = payload.get("output") {
            body["tool_response"] = json!({"output": output, "success": payload.get("success")});
        }
        body
    }

    async fn execute(&self, workspace: &Path, body: &Value) -> Result<Outcome, String> {
        let program = crate::shell::program().map_err(|error| error.to_string())?;
        let mut command = Command::new(program);
        command
            .env_clear()
            .envs(crate::tools::command::command_env())
            .args(crate::shell::arguments(&self.command, workspace))
            .current_dir(workspace)
            .env("CLAUDE_PROJECT_DIR", workspace)
            .envs(crate::prompt::summonr_home().map(|home| ("SUMMONR_HOME", home)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);
        if let Some(root) = &self.root {
            command
                .env("CLAUDE_PLUGIN_ROOT", root)
                .env("SUMMONR_PLUGIN_ROOT", root);
            if let Some(data) = root
                .file_name()
                .and_then(|name| crate::plugins::data_dir(&name.to_string_lossy()).ok())
            {
                command
                    .env("CLAUDE_PLUGIN_DATA", &data)
                    .env("SUMMONR_PLUGIN_DATA", &data);
            }
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("hook `{}` did not start: {error}", self.command))?;
        let group = child
            .id()
            .map(|pid| crate::tools::command::ProcessGroup::new(pid, false));
        let text = body.to_string();
        let work = async move {
            let stdin = child.stdin.take();
            let feed = async move {
                if let Some(mut stdin) = stdin {
                    drop(stdin.write_all(text.as_bytes()).await);
                }
            };
            let ((), stdout, stderr) = tokio::join!(
                feed,
                read_capped(child.stdout.take()),
                read_capped(child.stderr.take())
            );
            let status = child.wait().await?;
            Ok::<_, std::io::Error>(Outcome {
                code: status.code(),
                stdout,
                stderr,
            })
        };
        let finished = tokio::time::timeout(crate::limits::limits().job_wait(), work).await;
        drop(group);
        finished
            .map_err(|_elapsed| {
                format!(
                    "hook `{}` ran past {}s",
                    self.command,
                    crate::limits::limits().job_wait_secs
                )
            })?
            .map_err(|error| format!("hook `{}` failed: {error}", self.command))
    }
}

fn mcp_name(tool: &str) -> String {
    if tool.contains("__") {
        format!("mcp__{tool}")
    } else {
        tool.to_string()
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Hooks {
    lists: [Vec<Hook>; 4],
}

impl Hooks {
    #[must_use]
    pub(crate) fn of(&self, event: HookEvent) -> &[Hook] {
        &self.lists[index(event)]
    }

    pub(crate) fn push(&mut self, event: HookEvent, hook: Hook) {
        self.lists[index(event)].push(hook);
    }

    #[must_use]
    pub(crate) fn with_plugins(mut self, plugins: &[crate::plugins::Plugin]) -> Self {
        for plugin in plugins {
            let extra = plugin.hooks();
            for event in HookEvent::ALL {
                for hook in extra.of(event) {
                    self.push(event, hook.clone());
                }
            }
        }
        self
    }

    #[must_use]
    pub(crate) fn names(&self) -> Vec<&'static str> {
        HookEvent::ALL
            .into_iter()
            .filter(|event| !self.of(*event).is_empty())
            .map(HookEvent::key)
            .collect()
    }
}

const fn index(event: HookEvent) -> usize {
    match event {
        HookEvent::PreTool => 0,
        HookEvent::PostTool => 1,
        HookEvent::Compact => 2,
        HookEvent::Stop => 3,
    }
}

pub(crate) enum Verdict {
    Allow,
    Block(String),
    Note(String),
}

pub(crate) struct Call<'a> {
    pub(crate) event: HookEvent,
    pub(crate) workspace: &'a Path,
    pub(crate) session: &'a str,
    pub(crate) payload: &'a Value,
}

pub(crate) async fn run(hook: &Hook, call: &Call<'_>) -> Verdict {
    let body = match hook.dialect {
        Dialect::Summonr => call.payload.clone(),
        Dialect::Claude => hook.claude_payload(call),
    };
    let outcome = match hook.execute(call.workspace, &body).await {
        Ok(outcome) => outcome,
        Err(error) if call.event == HookEvent::PreTool => return Verdict::Block(error),
        Err(error) => return Verdict::Note(error),
    };
    judge(hook.dialect, call.event, outcome)
}

struct Outcome {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

async fn read_capped(pipe: Option<impl tokio::io::AsyncRead + Unpin>) -> String {
    let Some(mut pipe) = pipe else {
        return String::new();
    };
    let mut bytes = Vec::new();
    drop((&mut pipe).take(OUTPUT_MAX).read_to_end(&mut bytes).await);
    drop(tokio::io::copy(&mut pipe, &mut tokio::io::sink()).await);
    String::from_utf8_lossy(&bytes).trim().to_string()
}

fn judge(dialect: Dialect, event: HookEvent, outcome: Outcome) -> Verdict {
    let reason = if outcome.stderr.is_empty() {
        format!("exited with {:?}", outcome.code)
    } else {
        outcome.stderr.clone()
    };
    match (dialect, outcome.code) {
        (Dialect::Summonr, Some(0)) => Verdict::Allow,
        (Dialect::Summonr, Some(_) | None) => Verdict::Block(reason),
        (Dialect::Claude, Some(2)) if event == HookEvent::PreTool => Verdict::Block(reason),
        (Dialect::Claude, Some(0)) => {
            denied(&outcome.stdout).map_or(Verdict::Allow, Verdict::Block)
        }
        (Dialect::Claude, Some(_) | None) => Verdict::Note(reason),
    }
}

fn denied(stdout: &str) -> Option<String> {
    let value: Value = serde_json::from_str(stdout).ok()?;
    let output = value.get("hookSpecificOutput")?;
    (output.get("permissionDecision")?.as_str()? == "deny").then(|| {
        output
            .get("permissionDecisionReason")
            .and_then(Value::as_str)
            .unwrap_or("denied by a plugin hook")
            .to_string()
    })
}
