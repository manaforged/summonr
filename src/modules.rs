use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::config;
use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModuleId {
    Search,
    Lsp,
    Jobs,
    Task,
    Skills,
    Mcp,
    Mail,
    Browser,
    Memory,
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
            Self::Browser => "browser",
            Self::Memory => "memory",
        }
    }

    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        ALL.iter()
            .map(|module| module.id)
            .find(|id| id.name() == name)
    }
}

pub(crate) fn switch_map<'de, D>(
    deserializer: D,
) -> std::result::Result<BTreeMap<ModuleId, bool>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: BTreeMap<String, bool> = serde::Deserialize::deserialize(deserializer)?;
    Ok(raw
        .into_iter()
        .filter_map(|(name, on)| ModuleId::from_name(&name).map(|id| (id, on)))
        .collect())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Module {
    pub id: ModuleId,
    pub summary: &'static str,
    pub default: bool,
}

pub(crate) const ALL: [Module; 9] = [
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
        summary: "tools from MCP servers you named; each lists its tools once, then starts on first call and stops when idle",
        default: true,
    },
    Module {
        id: ModuleId::Mail,
        summary: "the model's mail tools: agents, send, inbox, board",
        default: false,
    },
    Module {
        id: ModuleId::Browser,
        summary: "drive a real Chrome: snapshot, click, type, tabs, logins kept",
        default: false,
    },
    Module {
        id: ModuleId::Memory,
        summary: "user and project memory files loaded into every trusted session",
        default: true,
    },
];

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Switches {
    off: BTreeSet<ModuleId>,
    denied: BTreeSet<ModuleId>,
}

impl Switches {
    pub fn load(workspace: &Path) -> Result<Self> {
        let user = config::file()?.modules.clone();
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
        user: &BTreeMap<ModuleId, bool>,
        project: &BTreeMap<ModuleId, bool>,
    ) -> Self {
        let mut off = BTreeSet::new();
        for module in ALL {
            if !user.get(&module.id).copied().unwrap_or(module.default) {
                off.insert(module.id);
            }
        }
        let denied = project
            .iter()
            .filter(|(_, on)| !**on)
            .map(|(id, _)| *id)
            .collect();
        Self { off, denied }
    }

    #[must_use]
    pub fn enabled(&self, id: ModuleId) -> bool {
        !self.off.contains(&id) && !self.denied.contains(&id)
    }

    #[must_use]
    pub fn table(&self) -> String {
        ALL.iter()
            .map(|module| {
                let state = if self.denied.contains(&module.id) {
                    "off (project)"
                } else if self.off.contains(&module.id) {
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
        .find(|module| ModuleId::from_name(name) == Some(module.id))
        .ok_or_else(|| {
            Error::Config(format!(
                "unknown module `{name}`; `summonr modules` lists them"
            ))
        })
}

pub fn set(id: ModuleId, on: bool) -> Result<()> {
    config::save_module(&config::path()?, id, on)
}
