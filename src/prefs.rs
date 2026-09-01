use std::collections::BTreeMap;
use std::path::Path;

use crate::config;
use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
pub enum Hotkey {
    Options,
    Newline,
    Editor,
    Files,
    Ledger,
    Diff,
    Pane,
}

impl Hotkey {
    pub const ALL: [Self; 7] = [
        Self::Options,
        Self::Newline,
        Self::Editor,
        Self::Files,
        Self::Ledger,
        Self::Diff,
        Self::Pane,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Options => "options",
            Self::Newline => "newline",
            Self::Editor => "editor",
            Self::Files => "files",
            Self::Ledger => "ledger",
            Self::Diff => "diff",
            Self::Pane => "pane",
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
            keys: ['o', 'j', 'g', 'e', 'l', 'x', 'p'],
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

    pub fn assign(&mut self, action: Hotkey, key: char) -> bool {
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
    key.is_ascii_lowercase() && !matches!(key, 'c' | 'd' | 'h' | 'i' | 'm' | 'q' | 's' | 'z')
}

fn parse(binding: &str) -> Option<char> {
    let mut chars = binding.strip_prefix("ctrl+")?.chars();
    let key = chars.next()?;
    (chars.next().is_none() && available(key)).then_some(key)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolOutput {
    Compact,
    Full,
}

impl ToolOutput {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefs {
    pub theme: String,
    pub splash: bool,
    pub animations: bool,
    pub reasoning: bool,
    pub tools: ToolOutput,
    pub bus: bool,
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
            reasoning: true,
            tools: ToolOutput::Compact,
            bus: true,
            hotkeys: Hotkeys::default(),
            last_endpoint: None,
            layout: "chat".to_string(),
        }
    }
}

#[cfg(test)]
const KEYS: [&str; 8] = [
    "theme",
    "splash",
    "animations",
    "reasoning",
    "tools",
    "bus",
    "layout",
    "hotkeys",
];

pub fn load_from(path: &Path) -> Result<Prefs> {
    let file = config::load_from(path)?;
    let mut prefs = Prefs {
        hotkeys: Hotkeys::configured(&file.hotkeys)?,
        ..Prefs::default()
    };
    if let Some(theme) = file.theme {
        prefs.theme = theme;
    }
    if let Some(splash) = file.splash {
        prefs.splash = splash;
    }
    if let Some(animations) = file.animations {
        prefs.animations = animations;
    }
    if let Some(reasoning) = file.reasoning {
        prefs.reasoning = reasoning;
    }
    if let Some(tools) = file.tools {
        prefs.tools = if tools == "full" {
            ToolOutput::Full
        } else {
            ToolOutput::Compact
        };
    }
    if let Some(layout) = file.layout {
        prefs.layout = layout;
    }
    if let Some(bus) = file.bus {
        prefs.bus = bus;
    }
    if let Some(last) = file.last_endpoint {
        prefs.last_endpoint = Some(last);
    }
    Ok(prefs)
}

pub fn save_in(path: &Path, prefs: &Prefs) -> Result<()> {
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

#[cfg(test)]
#[path = "prefs_tests.rs"]
mod tests;
