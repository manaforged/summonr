use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::config;
use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleId {
    Search,
    Lsp,
    Jobs,
    Task,
    Skills,
    Mcp,
    Mail,
}

impl ModuleId {
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Search => "search",
            Self::Lsp => "lsp",
            Self::Jobs => "jobs",
            Self::Task => "task",
            Self::Skills => "skills",
            Self::Mcp => "mcp",
            Self::Mail => "mail",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Module {
    pub id: ModuleId,
    pub summary: &'static str,
    pub default: bool,
}

pub(crate) const ALL: [Module; 7] = [
    Module {
        id: ModuleId::Search,
        summary: "the web tool (search, fetch) through Leyline with a browser identity",
        default: false,
    },
    Module {
        id: ModuleId::Lsp,
        summary: "diagnostics and definition from the curated language modules",
        default: true,
    },
    Module {
        id: ModuleId::Jobs,
        summary: "background commands that outlive a round; the turn waits or defers",
        default: true,
    },
    Module {
        id: ModuleId::Task,
        summary: "child sessions on the same seat: batch, background, poll, kill",
        default: true,
    },
    Module {
        id: ModuleId::Skills,
        summary: "the skill catalog in the system prompt and the skill tool",
        default: true,
    },
    Module {
        id: ModuleId::Mcp,
        summary: "tools from MCP servers you named; each starts on first call and stops when idle",
        default: true,
    },
    Module {
        id: ModuleId::Mail,
        summary: "the model's mail tools: agents, send, inbox, board",
        default: false,
    },
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Switches {
    off: BTreeSet<String>,
    denied: BTreeSet<String>,
}

impl Switches {
    pub fn load(workspace: &Path) -> Result<Self> {
        let user = config::file()?.modules;
        let path = workspace.join(".summonr/config.toml");
        let project = if path.is_file() {
            config::parse(&path)?.modules
        } else {
            BTreeMap::new()
        };
        Ok(Self::from_maps(&user, &project))
    }

    #[must_use]
    pub(crate) fn from_maps(
        user: &BTreeMap<String, bool>,
        project: &BTreeMap<String, bool>,
    ) -> Self {
        let mut off = BTreeSet::new();
        for module in ALL {
            let on = user
                .get(module.id.name())
                .copied()
                .unwrap_or(module.default);
            if !on {
                off.insert(module.id.name().to_string());
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
    pub fn enabled(&self, id: ModuleId) -> bool {
        !self.off.contains(id.name()) && !self.denied.contains(id.name())
    }

    #[must_use]
    pub fn table(&self) -> String {
        ALL.iter()
            .map(|module| {
                let state = if self.denied.contains(module.id.name()) {
                    "off (project)"
                } else if self.off.contains(module.id.name()) {
                    "off"
                } else {
                    "on"
                };
                format!("{:<7} {state:<40} {}", module.id.name(), module.summary)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

pub fn named(name: &str) -> Result<Module> {
    ALL.iter()
        .copied()
        .find(|module| module.id.name() == name)
        .ok_or_else(|| {
            Error::Config(format!(
                "unknown module `{name}`; `summonr modules` lists them"
            ))
        })
}

pub fn set(name: &str, on: bool) -> Result<()> {
    named(name)?;
    config::save_module(&config::path()?, name, on)
}
