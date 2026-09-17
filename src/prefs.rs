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
    Copy,
    Select,
    Pane,
    Search,
    History,
}

impl Hotkey {
    pub const ALL: [Self; 11] = [
        Self::Options,
        Self::Newline,
        Self::Editor,
        Self::Files,
        Self::Ledger,
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
            keys: ['o', 'j', 'g', 'e', 'l', 'x', 'b', 't', 'p', 'f', 'r'],
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Reasoning {
    Off,
    #[default]
    Collapsed,
    Expanded,
}

impl Reasoning {
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
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "off" | "false" | "hidden" => Self::Off,
            "expanded" | "on" | "true" | "shown" => Self::Expanded,
            _ => Self::Collapsed,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Receipts {
    #[default]
    Off,
    Collapsed,
    Expanded,
}

impl Receipts {
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
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "collapsed" => Self::Collapsed,
            "expanded" | "on" | "true" => Self::Expanded,
            _ => Self::Off,
        }
    }
}

/// Frames per second for the interface. The render loop blocks on input for
/// one frame budget, so this is the ceiling on redraws per second.
pub const FPS_DEFAULT: u32 = 25;
pub const FPS_MIN: u32 = 1;
pub const FPS_MAX: u32 = 240;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefs {
    pub theme: String,
    pub splash: bool,
    pub animations: bool,
    pub fps: u32,
    pub reasoning: Reasoning,
    pub receipts: Receipts,
    pub tools: ToolOutput,
    pub bus: bool,
    pub ban_comments: bool,
    pub ban_tests: bool,
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
            reasoning: Reasoning::Collapsed,
            receipts: Receipts::Off,
            tools: ToolOutput::Compact,
            bus: true,
            ban_comments: false,
            ban_tests: false,
            hotkeys: Hotkeys::default(),
            last_endpoint: None,
            layout: "chat".to_string(),
        }
    }
}

pub fn load_from(path: &Path) -> Result<Prefs> {
    let file = config::load_from(path)?;
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
            prefs.reasoning = Reasoning::Expanded;
        }
        Some(crate::config::ReasoningSetting::Flag(false)) => prefs.reasoning = Reasoning::Off,
        Some(crate::config::ReasoningSetting::Text(text)) => {
            prefs.reasoning = Reasoning::parse(&text);
        }
        None => {}
    }
    if let Some(receipts) = file.receipts.as_deref() {
        prefs.receipts = Receipts::parse(receipts);
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
    if let Some(ban) = file.ban_comments {
        prefs.ban_comments = ban;
    }
    if let Some(ban) = file.ban_tests {
        prefs.ban_tests = ban;
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

pub(crate) fn banned_edit(
    dest: &Path,
    before: &str,
    after: &str,
) -> Option<(&'static str, String)> {
    let prefs = load().unwrap_or_default();
    if prefs.ban_tests && test_surface(dest, before, after) {
        return Some(("tests", dest.display().to_string()));
    }
    if prefs.ban_comments
        && let Some(line) = added_comment(dest, before, after)
    {
        return Some(("comments", line));
    }
    None
}

fn test_surface(dest: &Path, before: &str, after: &str) -> bool {
    let name = dest
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let path = dest.to_string_lossy();
    if name.ends_with("_tests.rs") || name.starts_with("test_") || path.contains("/tests/") {
        return true;
    }
    ["#[test]", "#[cfg(test)]"]
        .iter()
        .any(|marker| after.contains(marker) && !before.contains(marker))
}

fn added_comment(dest: &Path, before: &str, after: &str) -> Option<String> {
    let extension = dest
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default();
    let markers: &[&str] = if matches!(
        extension,
        "rs" | "ts" | "tsx" | "js" | "go" | "wgsl" | "glsl"
    ) {
        &["//", "/*"]
    } else if matches!(extension, "py" | "sh" | "bash" | "zsh") {
        &["#"]
    } else {
        return None;
    };
    let have: std::collections::HashSet<&str> = before.lines().collect();
    for line in after.lines() {
        let text = line.trim_start();
        let directive =
            text.starts_with("#!") || text.contains("coding:") || text.contains("coding=");
        if !directive
            && markers.iter().any(|marker| text.starts_with(marker))
            && !have.contains(line)
        {
            return Some(text.chars().take(80).collect());
        }
    }
    None
}
