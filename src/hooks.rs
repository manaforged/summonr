use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

use crate::process::{Cap, Env, Process, Spec, Tail};

use crate::tools::Builtin;

mod config;

#[must_use]
pub(crate) fn builtin(name: &str) -> Option<Builtin> {
    let name = name.trim();
    Builtin::from_claude(name).or_else(|| Builtin::live(name))
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
                && Builtin::live(tool)
                    .is_some_and(|kind| kind.claude().1.iter().any(|alias| matcher.matches(alias))))
    }

    pub(crate) fn tool_name(&self, tool: &str) -> String {
        if self.dialect == Dialect::Summonr {
            return tool.to_string();
        }
        Builtin::live(tool)
            .and_then(|kind| kind.claude().0)
            .map_or_else(|| mcp_name(tool), str::to_string)
    }

    fn claude_payload(&self, call: &Call<'_>) -> Value {
        let payload = call.payload;
        let mut body = json!({
            "session_id": call.session,
            "cwd": call.workspace,
            "hook_event_name": call.event.claude(),
        });
        if let Some(tool) = payload.get("tool").and_then(Value::as_str) {
            let arguments = payload.get("arguments").cloned().unwrap_or(Value::Null);
            let by_name =
                Builtin::live(tool) == Some(Builtin::Search) && arguments.get("pattern").is_none();
            body["tool_name"] = json!(if by_name {
                Builtin::Search
                    .claude()
                    .1
                    .first()
                    .map_or_else(|| self.tool_name(tool), |alias| (*alias).to_string())
            } else {
                self.tool_name(tool)
            });
            body["tool_input"] = arguments;
            body["tool_use_id"] = payload.get("id").cloned().unwrap_or(Value::Null);
        }
        if let Some(output) = payload.get("output") {
            body["tool_response"] = json!({"output": output, "success": payload.get("success")});
        }
        body
    }

    async fn execute(&self, workspace: &Path, body: &Value) -> Result<Outcome, String> {
        let program = crate::shell::program().map_err(|error| error.to_string())?;
        let root = self.root.as_ref();
        let data = root
            .and_then(|root| root.file_name())
            .and_then(|name| crate::plugins::data_dir(&name.to_string_lossy()).ok());
        let spec = Spec {
            cwd: Some(workspace.to_path_buf()),
            base: Env::Clean,
            ..Spec::new(program, crate::shell::arguments(&self.command, workspace))
        }
        .envs([("CLAUDE_PROJECT_DIR", workspace.as_os_str())])
        .envs(crate::prompt::summonr_home().map(|home| ("SUMMONR_HOME", home)))
        .envs(
            root.into_iter()
                .flat_map(|root| [("CLAUDE_PLUGIN_ROOT", root), ("SUMMONR_PLUGIN_ROOT", root)]),
        )
        .envs(
            data.iter()
                .flat_map(|data| [("CLAUDE_PLUGIN_DATA", data), ("SUMMONR_PLUGIN_DATA", data)]),
        );
        let Process {
            mut child,
            mut stdin,
            stdout,
            stderr,
            group,
        } = crate::process::start(spec)
            .map_err(|error| format!("hook `{}` did not start: {error}", self.command))?;
        let text = body.to_string();
        let max = crate::limits::limits().hook_output_max_bytes;
        let mut stdout = Tail::spawn(Some(stdout), Cap::Head(max));
        let mut stderr = Tail::spawn(stderr, Cap::Head(max));
        let work = async move {
            drop(stdin.write_all(text.as_bytes()).await);
            drop(stdin);
            let status = child.wait().await?;
            drop(stdout.settle().await);
            drop(stderr.settle().await);
            Ok::<_, std::io::Error>(Outcome {
                code: status.code(),
                stdout: stdout.text().trim().to_string(),
                stderr: stderr.text().trim().to_string(),
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
