use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::hooks::Hooks;
use crate::mcp::SEPARATOR;
use summonr_core::dirs::{claude, user_home};
use summonr_types::mcp::CLAUDE_PREFIX;

const SHARED: &str = "settings.json";
const LOCAL: &str = "settings.local.json";
const PERMISSIONS: &str = "permissions";
const BASH: &str = "Bash";
const DEFAULT_MODES: [&str; 2] = ["default", "manual"];
const OWN_PREFIXES: [&str; 2] = ["SUMMONR_", "XDG_"];
const CLAUDE_DIRS: [&str; 2] = ["CLAUDE_CONFIG_DIR", "CLAUDE_CODE_TMPDIR"];

/// What the Claude Code settings files give a session, mapped onto summonr's hooks, policy, environment, and style.
#[derive(Clone, Debug, Default)]
pub(crate) struct Settings {
    pub(crate) read: Vec<PathBuf>,
    pub(crate) hooks: Hooks,
    pub(crate) deny: Vec<String>,
    pub(crate) ask: Vec<String>,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) style: Option<String>,
    pub(crate) notes: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Layer {
    User,
    Project,
    Local,
}

impl Layer {
    const ALL: [Self; 3] = [Self::User, Self::Project, Self::Local];

    fn path(self, workspace: &Path) -> Option<PathBuf> {
        match self {
            Self::User => Some(claude(&user_home()?).join(SHARED)),
            Self::Project => Some(claude(workspace).join(SHARED)),
            Self::Local => Some(claude(workspace).join(LOCAL)),
        }
    }

    const fn needs_trust(self) -> bool {
        match self {
            Self::User => false,
            Self::Project | Self::Local => true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum List {
    Deny,
    Ask,
}

impl List {
    const ALL: [Self; 2] = [Self::Deny, Self::Ask];

    const fn key(self) -> &'static str {
        match self {
            Self::Deny => "deny",
            Self::Ask => "ask",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Rule {
    Command(String),
    Tool(String),
}

/// The settings for `workspace` when `import_claude_settings = true`: user, then project and local in a trusted workspace.
pub(crate) fn load(workspace: &Path, trust: bool) -> Settings {
    let mut settings = Settings::default();
    let on = crate::config::file().is_ok_and(|file| file.import_claude_settings == Some(true));
    if !on {
        return settings;
    }
    for layer in Layer::ALL {
        let Some(path) = layer
            .path(workspace)
            .filter(|_| trust || !layer.needs_trust())
        else {
            continue;
        };
        if settings.read.contains(&path) {
            continue;
        }
        match crate::plugins::read_parsed(&path, |text: &str| serde_json::from_str::<Value>(text)) {
            Ok(Some(doc)) => {
                settings.merge(&doc);
                settings.read.push(path);
            }
            Ok(None) => {}
            Err(error) => settings.notes.push(format!(
                "{error}; summonr skipped this Claude Code settings file"
            )),
        }
    }
    settings
}

/// The Claude Code settings files a session in `workspace` reads.
pub fn files(workspace: &Path, trust: bool) -> Vec<PathBuf> {
    load(workspace, trust).read
}

impl Settings {
    /// Prints each note once per process.
    pub(crate) fn report(&self) {
        for note in &self.notes {
            crate::display::note(note);
        }
    }

    pub(super) fn merge(&mut self, doc: &Value) {
        self.hooks.extend(&crate::plugins::claude_hooks(doc));
        self.notes.extend(
            crate::plugins::unsupported_hooks(doc)
                .into_iter()
                .map(|what| format!("{what} has no summonr hook; summonr skipped it")),
        );
        self.permissions(&doc[PERMISSIONS]);
        for (name, value) in doc["env"].as_object().into_iter().flatten() {
            self.variable(name, value);
        }
        if let Some(style) = doc["outputStyle"].as_str() {
            self.style = Some(style.to_string());
        }
    }

    fn permissions(&mut self, permissions: &Value) {
        for list in List::ALL {
            for raw in strings(&permissions[list.key()]) {
                self.permit(list, raw);
            }
        }
        let allowed = strings(&permissions["allow"]).count();
        if allowed > 0 {
            self.notes.push(format!(
                "summonr skipped {allowed} permissions.allow rule(s): a summonr allow list blocks every command not on it, so it cannot grant what Claude Code allows"
            ));
        }
        if let Some(mode) = permissions["defaultMode"].as_str()
            && !DEFAULT_MODES.contains(&mode)
        {
            self.notes.push(format!(
                "permissions.defaultMode `{mode}` has no summonr session mode; summonr skipped it"
            ));
        }
    }

    fn variable(&mut self, name: &str, value: &Value) {
        match value.as_str() {
            Some(_) if protected(name) => self.notes.push(format!(
                "env `{name}` is a variable summonr sets itself; summonr skipped it"
            )),
            Some(value) => {
                self.env.retain(|(have, _)| have != name);
                self.env.push((name.to_string(), value.to_string()));
            }
            None => self
                .notes
                .push(format!("env `{name}` is not a string; summonr skipped it")),
        }
    }

    fn permit(&mut self, list: List, raw: &str) {
        match (list, rule(raw)) {
            (List::Deny, Some(Rule::Command(pattern) | Rule::Tool(pattern))) => {
                self.deny.push(pattern);
            }
            (List::Ask, Some(Rule::Command(pattern))) => self.ask.push(pattern),
            (List::Ask, Some(Rule::Tool(_))) | (List::Deny | List::Ask, None) => {
                self.notes.push(format!(
                    "permissions.{} rule `{raw}` has no matching summonr policy rule; summonr skipped it",
                    list.key()
                ));
            }
        }
    }
}

/// The summonr policy pattern for a Claude Code deny or ask rule, when one matches at least what it matches.
pub(super) fn rule(raw: &str) -> Option<Rule> {
    let raw = raw.trim();
    if let Some(name) = raw.strip_prefix(CLAUDE_PREFIX) {
        return tool(name).map(Rule::Tool);
    }
    let inner = raw.strip_prefix(BASH)?.trim_start().strip_prefix('(')?;
    let inner = inner.strip_suffix(')')?.trim();
    let prefix = inner
        .strip_suffix(":*")
        .or_else(|| inner.strip_suffix(" *"))
        .unwrap_or(inner)
        .trim();
    (!prefix.is_empty() && !prefix.contains('*')).then(|| Rule::Command(prefix.to_string()))
}

fn tool(name: &str) -> Option<String> {
    let (server, tool) = name.split_once(SEPARATOR).unwrap_or((name, "*"));
    let plain = |text: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
    };
    let stem = tool.strip_suffix('*').unwrap_or(tool);
    (plain(server) && (stem.is_empty() || plain(stem)))
        .then(|| format!("{server}{SEPARATOR}{tool}"))
}

fn protected(name: &str) -> bool {
    OWN_PREFIXES.iter().any(|prefix| name.starts_with(prefix))
        || crate::tools::command::KEEP.contains(&name)
        || crate::askpass::VARS.contains(&name)
        || crate::plugins::HOOK_VARS.contains(&name)
        || CLAUDE_DIRS.contains(&name)
}

fn strings(value: &Value) -> impl Iterator<Item = &str> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
}
