use crate::catalog::Builtin;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[path = "hooks_config.rs"]
mod config;
#[path = "hooks_kinds.rs"]
mod kinds;

pub use kinds::{Compaction, End, Failure, Notice, Permission, Start};

/// The built-in tool that a Claude Code or summonr tool name names.
#[must_use]
pub fn builtin(name: &str) -> Option<Builtin> {
    let name = name.trim();
    Builtin::from_claude(name).or_else(|| Builtin::live(name))
}

/// The points in a session where hooks run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookEvent {
    SessionStart,
    Prompt,
    PreTool,
    PermissionRequest,
    PostTool,
    ToolFailure,
    Notification,
    SubagentStart,
    SubagentStop,
    PreCompact,
    Compact,
    Stop,
    StopFailure,
    SessionEnd,
}

impl HookEvent {
    /// Every hook event, in order.
    pub const ALL: [Self; 14] = [
        Self::SessionStart,
        Self::Prompt,
        Self::PreTool,
        Self::PermissionRequest,
        Self::PostTool,
        Self::ToolFailure,
        Self::Notification,
        Self::SubagentStart,
        Self::SubagentStop,
        Self::PreCompact,
        Self::Compact,
        Self::Stop,
        Self::StopFailure,
        Self::SessionEnd,
    ];
    /// The event's key in the `[hooks]` config table.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::SessionStart => "session_start",
            Self::Prompt => "user_prompt",
            Self::PreTool => "pre_tool",
            Self::PermissionRequest => "permission_request",
            Self::PostTool => "post_tool",
            Self::ToolFailure => "post_tool_failure",
            Self::Notification => "notification",
            Self::SubagentStart => "subagent_start",
            Self::SubagentStop => "subagent_stop",
            Self::PreCompact => "pre_compact",
            Self::Compact => "compact",
            Self::Stop => "stop",
            Self::StopFailure => "stop_failure",
            Self::SessionEnd => "session_end",
        }
    }
    /// The event's Claude Code hook name.
    #[must_use]
    pub const fn claude(self) -> &'static str {
        match self {
            Self::SessionStart => "SessionStart",
            Self::Prompt => "UserPromptSubmit",
            Self::PreTool => "PreToolUse",
            Self::PermissionRequest => "PermissionRequest",
            Self::PostTool => "PostToolUse",
            Self::ToolFailure => "PostToolUseFailure",
            Self::Notification => "Notification",
            Self::SubagentStart => "SubagentStart",
            Self::SubagentStop => "SubagentStop",
            Self::PreCompact => "PreCompact",
            Self::Compact => "PostCompact",
            Self::Stop => "Stop",
            Self::StopFailure => "StopFailure",
            Self::SessionEnd => "SessionEnd",
        }
    }
    const fn slot(self) -> usize {
        self as usize
    }
}

/// The input shape a hook command reads: summonr or Claude Code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Dialect {
    #[default]
    Summonr,
    Claude,
}

/// Which tools a hook applies to: a name list or a pattern.
#[derive(Clone, Debug)]
pub enum Matcher {
    Names(Vec<String>),
    Pattern(regex::Regex),
}

impl Matcher {
    /// The matcher in `text`; empty or `*` matches every tool.
    pub fn parse(text: &str) -> Option<Self> {
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

/// One hook command with its dialect, matcher, and time limit.
#[derive(Clone, Debug)]
pub struct Hook {
    pub command: String,
    pub dialect: Dialect,
    pub matcher: Option<Matcher>,
    pub root: Option<PathBuf>,
    pub timeout: Option<Duration>,
}

impl Hook {
    fn plain(command: String, dialect: Dialect) -> Self {
        Self {
            command,
            dialect,
            matcher: None,
            root: None,
            timeout: None,
        }
    }
    /// The command hook in one Claude Code `hooks.json` handler; `None` for another type.
    #[must_use]
    pub fn claude(handler: &Value, matcher: Option<Matcher>) -> Option<Self> {
        if handler["type"] != "command" {
            return None;
        }
        Some(Self {
            matcher,
            timeout: handler["timeout"].as_f64().and_then(seconds),
            ..Self::plain(handler["command"].as_str()?.to_string(), Dialect::Claude)
        })
    }
    /// Whether this hook runs for `subject`: the tool, or the event's matcher value.
    pub fn applies(&self, subject: Option<&str>) -> bool {
        let (Some(matcher), Some(tool)) = (&self.matcher, subject) else {
            return true;
        };
        matcher.matches(&self.tool_name(tool))
            || (self.dialect != Dialect::Summonr
                && Builtin::live(tool)
                    .is_some_and(|kind| kind.claude().1.iter().any(|alias| matcher.matches(alias))))
    }
    /// The tool name in this hook's dialect.
    pub fn tool_name(&self, tool: &str) -> String {
        if self.dialect == Dialect::Summonr {
            return tool.to_string();
        }
        Builtin::live(tool)
            .and_then(|kind| kind.claude().0)
            .map_or_else(|| mcp_name(tool), str::to_string)
    }
    /// The Claude Code JSON input for this hook call.
    pub fn claude_payload(&self, call: &Call<'_>) -> Value {
        let payload = call.payload;
        let mut body = json!({
            "session_id": call.session,
            "transcript_path": call.transcript,
            "cwd": call.workspace,
            "permission_mode": call.permission.claude(),
            "hook_event_name": call.event.claude(),
        });
        for (from, to) in FIELDS {
            if let Some(value) = payload.get(from) {
                body[to] = value.clone();
            }
        }
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
            if let Some(id) = payload.get("id") {
                body["tool_use_id"] = id.clone();
            }
        }
        if let Some(output) = payload.get("output") {
            if call.event == HookEvent::ToolFailure {
                body["error"] = output.clone();
            } else {
                body["tool_response"] =
                    json!({ "output": output, "success": payload.get("success") });
            }
        }
        body
    }
}

const FIELDS: [(&str, &str); 17] = [
    ("source", "source"),
    ("model", "model"),
    ("prompt", "prompt"),
    ("stop_hook_active", "stop_hook_active"),
    ("text", "last_assistant_message"),
    ("trigger", "trigger"),
    ("custom_instructions", "custom_instructions"),
    ("summary", "compact_summary"),
    ("reason", "reason"),
    ("agent_id", "agent_id"),
    ("agent_type", "agent_type"),
    ("agent_transcript", "agent_transcript_path"),
    ("notification_type", "notification_type"),
    ("message", "message"),
    ("title", "title"),
    ("error", "error"),
    ("error_details", "error_details"),
];

fn seconds(value: f64) -> Option<Duration> {
    Duration::try_from_secs_f64(value).ok()
}

fn mcp_name(tool: &str) -> String {
    if tool.contains("__") {
        format!("{}{tool}", crate::mcp::CLAUDE_PREFIX)
    } else {
        tool.to_string()
    }
}

/// The child agent a session's hooks run in: its id and its agent name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subagent {
    pub id: String,
    pub name: String,
}

/// The configured hooks by event, and the child agent they run in.
#[derive(Clone, Debug, Default)]
pub struct Hooks {
    lists: [Vec<Hook>; HookEvent::ALL.len()],
    subagent: Option<Subagent>,
}

impl Hooks {
    /// The hooks for `event`.
    #[must_use]
    pub fn of(&self, event: HookEvent) -> &[Hook] {
        &self.lists[event.slot()]
    }
    /// Adds `hook` to `event`.
    pub fn push(&mut self, event: HookEvent, hook: Hook) {
        self.lists[event.slot()].push(hook);
    }
    /// Adds every hook in `other` after the hooks already set for its event.
    pub fn extend(&mut self, other: &Self) {
        for event in HookEvent::ALL {
            self.lists[event.slot()].extend(other.of(event).iter().cloned());
        }
    }
    /// Marks these hooks as child agent `id` of agent type `name`, so its stop runs `subagent_stop`.
    pub fn set_subagent(&mut self, id: String, name: String) {
        self.subagent = Some(Subagent { id, name });
    }
    /// The child agent these hooks run in, if any.
    #[must_use]
    pub const fn subagent(&self) -> Option<&Subagent> {
        self.subagent.as_ref()
    }
    /// The config keys of the events that have hooks.
    #[must_use]
    pub fn names(&self) -> Vec<&'static str> {
        HookEvent::ALL
            .into_iter()
            .filter(|event| !self.of(*event).is_empty())
            .map(HookEvent::key)
            .collect()
    }
}

/// One hook call: the event, session facts, and payload.
pub struct Call<'a> {
    pub event: HookEvent,
    pub workspace: &'a Path,
    pub session: &'a str,
    pub transcript: &'a Path,
    pub permission: Permission,
    pub payload: &'a Value,
    pub env: &'a [(String, String)],
}
