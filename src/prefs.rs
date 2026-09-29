use std::collections::BTreeMap;
use std::path::Path;

use crate::config;
use crate::error::{Error, Result};

mod diff;
mod edit;
pub use diff::DiffLayout;
pub(crate) use edit::banned_edit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Hotkey {
    Options,
    Newline,
    Editor,
    Files,
    Ledger,
    Agents,
    Diff,
    Copy,
    Select,
    Pane,
    Search,
    History,
}

impl Hotkey {
    pub const ALL: [Self; 12] = [
        Self::Options,
        Self::Newline,
        Self::Editor,
        Self::Files,
        Self::Ledger,
        Self::Agents,
        Self::Diff,
        Self::Copy,
        Self::Select,
        Self::Pane,
        Self::Search,
        Self::History,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Options => "options",
            Self::Newline => "newline",
            Self::Editor => "editor",
            Self::Files => "files",
            Self::Ledger => "ledger",
            Self::Agents => "agents",
            Self::Diff => "diff",
            Self::Copy => "copy",
            Self::Select => "select",
            Self::Pane => "pane",
            Self::Search => "search",
            Self::History => "history",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }

    fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.name() == name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hotkeys {
    keys: [char; Hotkey::ALL.len()],
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            keys: ['o', 'j', 'g', 'e', 'l', 'a', 'x', 'b', 't', 'p', 'f', 'r'],
        }
    }
}

impl Hotkeys {
    pub fn configured(rows: &BTreeMap<String, String>) -> Result<Self> {
        let mut hotkeys = Self::default();
        for (name, binding) in rows {
            let action = Hotkey::named(name)
                .ok_or_else(|| Error::Config(format!("config.toml: unknown hotkey `{name}`")))?;
            let key = parse(binding).ok_or_else(|| {
                Error::Config(format!(
                    "config.toml: hotkey `{name}` must be an available ctrl+letter"
                ))
            })?;
            hotkeys.keys[action.index()] = key;
        }
        if hotkeys.keys.iter().filter(|key| **key == 'y').count() > 1 {
            return Err(Error::Config(
                "config.toml: hotkey ctrl+y is assigned more than once".to_string(),
            ));
        }
        for action in Hotkey::ALL {
            let index = action.index();
            let inherited = matches!(action, Hotkey::Copy | Hotkey::Search | Hotkey::History)
                && !rows.contains_key(action.name());
            if hotkeys.keys[index] == 'y'
                || inherited
                    && hotkeys
                        .keys
                        .iter()
                        .enumerate()
                        .any(|(other, key)| other != index && *key == hotkeys.keys[index])
            {
                hotkeys.keys[index] = Self::default()
                    .keys
                    .into_iter()
                    .chain('a'..='z')
                    .find(|key| available(*key) && !hotkeys.keys.contains(key))
                    .ok_or_else(|| {
                        Error::Config(format!("no available {} hotkey", action.name()))
                    })?;
            }
        }
        for (index, key) in hotkeys.keys.iter().enumerate() {
            if hotkeys.keys[..index].contains(key) {
                return Err(Error::Config(format!(
                    "config.toml: hotkey ctrl+{key} is assigned more than once"
                )));
            }
        }
        Ok(hotkeys)
    }

    #[must_use]
    pub const fn key(&self, action: Hotkey) -> char {
        self.keys[action.index()]
    }

    #[must_use]
    pub fn label(&self, action: Hotkey) -> String {
        format!("ctrl+{}", self.key(action))
    }

    #[must_use]
    pub fn action(&self, key: char) -> Option<Hotkey> {
        Hotkey::ALL
            .into_iter()
            .find(|action| self.key(*action) == key)
    }

    pub(crate) fn assign(&mut self, action: Hotkey, key: char) -> bool {
        if !available(key) {
            return false;
        }
        let old = self.key(action);
        if let Some(other) = self.action(key) {
            self.keys[other.index()] = old;
        }
        self.keys[action.index()] = key;
        true
    }

    pub fn entries(&self) -> impl Iterator<Item = (Hotkey, String)> + '_ {
        Hotkey::ALL
            .into_iter()
            .map(|action| (action, self.label(action)))
    }
}

#[must_use]
pub const fn available(key: char) -> bool {
    key.is_ascii_lowercase()
        && !matches!(
            key,
            'c' | 'd' | 'h' | 'i' | 'm' | 'q' | 's' | 'v' | 'y' | 'z'
        )
}

fn parse(binding: &str) -> Option<char> {
    let mut chars = binding.strip_prefix("ctrl+")?.chars();
    let key = chars.next()?;
    (chars.next().is_none() && (available(key) || key == 'y')).then_some(key)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolOutput {
    Compact,
    Full,
}

impl ToolOutput {
    pub(crate) fn text(self, output: &str) -> &str {
        if self == Self::Full {
            return output;
        }
        let line = output
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("");
        let end = line
            .char_indices()
            .nth(120)
            .map_or(line.len(), |(index, _)| index);
        &line[..end]
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Full => "full",
        }
    }

    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::Compact => Self::Full,
            Self::Full => Self::Compact,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Visibility {
    Off,
    Collapsed,
    Expanded,
}

impl Visibility {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Collapsed => "collapsed",
            Self::Expanded => "expanded",
        }
    }

    #[must_use]
    pub fn toggle(self) -> Self {
        match self {
            Self::Off => Self::Collapsed,
            Self::Collapsed => Self::Expanded,
            Self::Expanded => Self::Off,
        }
    }

    #[must_use]
    pub fn parse(value: &str, fallback: Self) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "false" | "hidden" => Self::Off,
            "collapsed" => Self::Collapsed,
            "expanded" | "on" | "true" | "shown" => Self::Expanded,
            _ => fallback,
        }
    }
}

pub const FPS_DEFAULT: u32 = 25;
pub const FPS_MIN: u32 = 1;
pub const FPS_MAX: u32 = 240;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefs {
    pub theme: String,
    pub splash: bool,
    pub animations: bool,
    pub fps: u32,
    pub reasoning: Visibility,
    pub receipts: Visibility,
    pub tools: ToolOutput,
    pub diff_layout: DiffLayout,
    pub mail: bool,
    pub ban_comments: bool,
    pub ban_tests: bool,
    pub plan_mode: bool,
    pub layout: String,
    pub hotkeys: Hotkeys,
    pub last_endpoint: Option<String>,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: "space".to_string(),
            splash: true,
            animations: true,
            fps: FPS_DEFAULT,
            reasoning: Visibility::Collapsed,
            receipts: Visibility::Off,
            tools: ToolOutput::Compact,
            diff_layout: DiffLayout::Split,
            mail: true,
            ban_comments: false,
            ban_tests: false,
            plan_mode: false,
            hotkeys: Hotkeys::default(),
            last_endpoint: None,
            layout: "chat".to_string(),
        }
    }
}

pub(crate) fn load_from(path: &Path) -> Result<Prefs> {
    let file = config::parse(path)?;
    let mut prefs = Prefs {
        hotkeys: Hotkeys::configured(&file.hotkeys)?,
        ..Prefs::default()
    };
    if let Some(name) = file.theme {
        prefs.theme = name;
    }
    if let Some(splash) = file.splash {
        prefs.splash = splash;
    }
    if let Some(animations) = file.animations {
        prefs.animations = animations;
    }
    if let Some(fps) = file.fps {
        prefs.fps = fps;
    }
    match file.reasoning {
        Some(crate::config::ReasoningSetting::Flag(true)) => {
            prefs.reasoning = Visibility::Expanded;
        }
        Some(crate::config::ReasoningSetting::Flag(false)) => prefs.reasoning = Visibility::Off,
        Some(crate::config::ReasoningSetting::Text(text)) => {
            prefs.reasoning = Visibility::parse(&text, Visibility::Collapsed);
        }
        None => {}
    }
    if let Some(receipts) = file.receipts.as_deref() {
        prefs.receipts = Visibility::parse(receipts, Visibility::Off);
    }
    if let Some(tools) = file.tools {
        prefs.tools = if tools == "full" {
            ToolOutput::Full
        } else {
            ToolOutput::Compact
        };
    }
    if let Some(layout) = file.diff_layout {
        prefs.diff_layout = layout;
    }
    if let Some(layout) = file.layout {
        prefs.layout = layout;
    }
    if let Some(mail) = file.mail {
        prefs.mail = mail;
    }
    if let Some(ban) = file.ban_comments {
        prefs.ban_comments = ban;
    }
    if let Some(ban) = file.ban_tests {
        prefs.ban_tests = ban;
    }
    if let Some(plan) = file.plan_mode {
        prefs.plan_mode = plan;
    }
    if let Some(last) = file.last_endpoint {
        prefs.last_endpoint = Some(last);
    }
    Ok(prefs)
}

pub(crate) fn save_in(path: &Path, prefs: &Prefs) -> Result<()> {
    config::save_prefs(path, prefs)
}

pub fn set_last_endpoint(name: &str) {
    if let (Ok(mut prefs), Some(path)) = (load(), path()) {
        prefs.last_endpoint = Some(name.to_string());
        drop(save_in(&path, &prefs));
    }
}

#[must_use]
pub fn path() -> Option<std::path::PathBuf> {
    config::path().ok()
}

pub fn load() -> Result<Prefs> {
    load_from(&config::path()?)
}
