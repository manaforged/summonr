//! Interface preferences. Top-level keys in `~/.mage/config.toml`, kept
//! apart from the `[endpoint.*]` sections so the file stays hand-editable.

use std::path::Path;

use crate::config;
use crate::error::Result;

/// How much of a tool's output the transcript keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolOutput {
    /// The last line only.
    Compact,
    /// The last twelve lines.
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

/// What the interface remembers between runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefs {
    /// Splash theme name (`space` or `fantasy`).
    pub theme: String,
    /// Boot into the splash cover instead of the chat screen.
    pub splash: bool,
    /// Bloom, drift, and reveal animations.
    pub animations: bool,
    /// Show streamed reasoning in the transcript.
    pub reasoning: bool,
    pub tools: ToolOutput,
    /// Mail from the bus starts a turn while idle.
    pub bus: bool,
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
        }
    }
}

#[cfg(test)]
const KEYS: [&str; 6] = ["theme", "splash", "animations", "reasoning", "tools", "bus"];

/// Read the top-level preference keys from `path`. Missing keys keep
/// their defaults.
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
    Ok(prefs)
}

/// Write the preference keys at the top level of `path`, replacing the
/// ones already there and leaving every other line alone.
pub fn save_in(path: &Path, prefs: &Prefs) -> Result<()> {
    config::save_prefs(path, prefs)
}

/// `~/.mage/config.toml`, when the home directory is known.
#[must_use]
pub fn path() -> Option<std::path::PathBuf> {
    config::path().ok()
}

/// Load from `~/.mage/config.toml`.
pub fn load() -> Result<Prefs> {
    load_from(&config::path()?)
}

#[cfg(test)]
#[path = "prefs_tests.rs"]
mod tests;
