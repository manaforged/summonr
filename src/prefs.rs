use std::path::Path;

use crate::config;
use crate::error::Result;

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
            last_endpoint: None,
        }
    }
}

#[cfg(test)]
const KEYS: [&str; 6] = ["theme", "splash", "animations", "reasoning", "tools", "bus"];

pub fn load_from(path: &Path) -> Result<Prefs> {
    let file = config::load_from(path)?;
    let mut prefs = Prefs::default();
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
        let _ = save_in(&path, &prefs);
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
