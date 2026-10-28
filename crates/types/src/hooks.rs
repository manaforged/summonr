use crate::catalog::Builtin;
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[path = "hooks_config.rs"]
mod config;

#[must_use]
pub fn builtin(name: &str) -> Option<Builtin> {
    let name = name.trim();
    Builtin::from_claude(name).or_else(|| Builtin::live(name))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookEvent {
    PreTool,
    PostTool,
    Compact,
    Stop,
}

impl HookEvent {
    pub const ALL: [Self; 4] = [Self::PreTool, Self::PostTool, Self::Compact, Self::Stop];
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::PreTool => "pre_tool",
            Self::PostTool => "post_tool",
            Self::Compact => "compact",
            Self::Stop => "stop",
        }
    }
    #[must_use]
    pub const fn claude(self) -> &'static str {
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
pub enum Dialect {
    #[default]
    Summonr,
    Claude,
}

#[derive(Clone, Debug)]
pub enum Matcher {
    Names(Vec<String>),
    Pattern(regex::Regex),
}

impl Matcher {
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

#[derive(Clone, Debug)]
pub struct Hook {
    pub command: String,
    pub dialect: Dialect,
    pub matcher: Option<Matcher>,
    pub root: Option<PathBuf>,
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
    pub fn applies(&self, tool: Option<&str>) -> bool {
        let (Some(matcher), Some(tool)) = (&self.matcher, tool) else {
            return true;
        };
        matcher.matches(&self.tool_name(tool))
            || (self.dialect != Dialect::Summonr
                && Builtin::live(tool)
                    .is_some_and(|kind| kind.claude().1.iter().any(|alias| matcher.matches(alias))))
    }
    pub fn tool_name(&self, tool: &str) -> String {
        if self.dialect == Dialect::Summonr {
            return tool.to_string();
        }
        Builtin::live(tool)
            .and_then(|kind| kind.claude().0)
            .map_or_else(|| mcp_name(tool), str::to_string)
    }
    pub fn claude_payload(&self, call: &Call<'_>) -> Value {
        let payload = call.payload;
        let mut body = json!({ "session_id": call.session, "cwd": call.workspace, "hook_event_name": call.event.claude() });
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
            body["tool_response"] = json!({ "output": output, "success": payload.get("success") });
        }
        body
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
pub struct Hooks {
    lists: [Vec<Hook>; 4],
}

impl Hooks {
    #[must_use]
    pub fn of(&self, event: HookEvent) -> &[Hook] {
        &self.lists[index(event)]
    }
    pub fn push(&mut self, event: HookEvent, hook: Hook) {
        self.lists[index(event)].push(hook);
    }
    #[must_use]
    pub fn names(&self) -> Vec<&'static str> {
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

pub struct Call<'a> {
    pub event: HookEvent,
    pub workspace: &'a Path,
    pub session: &'a str,
    pub payload: &'a Value,
}
