use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::config;
use crate::error::{Error, Result};

/// One compiled-in capability with a switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Module {
    pub name: &'static str,
    pub summary: &'static str,
    pub default: bool,
}

pub const ALL: [Module; 7] = [
    Module {
        name: "search",
        summary: "web_search and web_fetch (DuckDuckGo without a key, Brave with one)",
        default: true,
    },
    Module {
        name: "lsp",
        summary: "diagnostics and definition from the curated language modules",
        default: true,
    },
    Module {
        name: "jobs",
        summary: "background commands that outlive a round; the turn waits or defers",
        default: true,
    },
    Module {
        name: "task",
        summary: "child sessions: batch, background, poll, kill",
        default: true,
    },
    Module {
        name: "skills",
        summary: "the skill catalog in the system prompt and the skill tool",
        default: true,
    },
    Module {
        name: "bus",
        summary: "mail and a blackboard between summoner sessions on this machine",
        default: false,
    },
    Module {
        name: "gui",
        summary: "the desktop window (`summon --gui`, a `--features gui` build)",
        default: true,
    },
];

/// Which modules are on: user config turns them on or off, a project may only turn them off.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Switches {
    off: BTreeSet<String>,
    denied: BTreeSet<String>,
}

impl Switches {
    pub fn load(workspace: &Path) -> Result<Self> {
        let user = config::load()?.modules;
        let path = workspace.join(".mage/config.toml");
        let project = if path.is_file() {
            config::load_from(&path)?.modules
        } else {
            BTreeMap::new()
        };
        Ok(Self::from_maps(&user, &project))
    }

    #[must_use]
    pub fn from_maps(user: &BTreeMap<String, bool>, project: &BTreeMap<String, bool>) -> Self {
        let mut off = BTreeSet::new();
        for module in ALL {
            let on = user.get(module.name).copied().unwrap_or(module.default);
            if !on {
                off.insert(module.name.to_string());
            }
        }
        let denied = project
            .iter()
            .filter(|(_, on)| !**on)
            .map(|(name, _)| name.clone())
            .collect();
        Self { off, denied }
    }

    #[must_use]
    pub fn enabled(&self, name: &str) -> bool {
        named(name).is_ok() && !self.off.contains(name) && !self.denied.contains(name)
    }

    #[must_use]
    pub fn table(&self) -> String {
        ALL.iter()
            .map(|module| {
                let state = if self.denied.contains(module.name) {
                    "off (project)"
                } else if self.off.contains(module.name) {
                    "off"
                } else if module.name == "gui" && !cfg!(feature = "gui") {
                    "unavailable (build with --features gui)"
                } else {
                    "on"
                };
                format!("{:<7} {state:<40} {}", module.name, module.summary)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub fn named(name: &str) -> Result<Module> {
    ALL.iter()
        .copied()
        .find(|module| module.name == name)
        .ok_or_else(|| {
            Error::Config(format!(
                "unknown module `{name}`; `summon modules` lists them"
            ))
        })
}

pub fn set(name: &str, on: bool) -> Result<()> {
    named(name)?;
    config::save_module(&config::path()?, name, on)
}

#[cfg(test)]
#[path = "modules_tests.rs"]
mod tests;
