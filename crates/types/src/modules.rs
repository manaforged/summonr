use std::collections::{BTreeMap, BTreeSet};
use summonr_core::error::{Error, Result};

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
        ALL[self as usize].name
    }

    #[must_use]
    fn from_name(name: &str) -> Option<Self> {
        ALL.iter()
            .find(|module| module.name == name)
            .map(|module| module.id)
    }
}

pub fn switch_map<'de, D>(
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
    pub name: &'static str,
    pub summary: &'static str,
    pub default: bool,
}

pub const ALL: [Module; 9] = [
    Module {
        id: ModuleId::Search,
        name: "search",
        summary: "the web tool (search, fetch) through Leyline with a browser identity",
        default: false,
    },
    Module {
        id: ModuleId::Lsp,
        name: "lsp",
        summary: "diagnostics and definition from the curated language modules",
        default: true,
    },
    Module {
        id: ModuleId::Jobs,
        name: "jobs",
        summary: "background commands that outlive a round; the turn waits or defers",
        default: true,
    },
    Module {
        id: ModuleId::Task,
        name: "task",
        summary: "child sessions using the same model: batch, background, poll, kill",
        default: true,
    },
    Module {
        id: ModuleId::Skills,
        name: "skills",
        summary: "the skill catalog in the system prompt and the skill tool",
        default: true,
    },
    Module {
        id: ModuleId::Mcp,
        name: "mcp",
        summary: "tools from MCP servers you named; each lists its tools once, then starts on first call and stops when idle",
        default: true,
    },
    Module {
        id: ModuleId::Mail,
        name: "mail",
        summary: "the model's mail tools: agents, send, inbox, board",
        default: false,
    },
    Module {
        id: ModuleId::Browser,
        name: "browser",
        summary: "drive a real Chrome: snapshot, click, type, tabs, logins kept",
        default: false,
    },
    Module {
        id: ModuleId::Memory,
        name: "memory",
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
    #[must_use]
    pub fn from_maps(user: &BTreeMap<ModuleId, bool>, project: &BTreeMap<ModuleId, bool>) -> Self {
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
                "unknown module `{name}`; `summonr modules` lists them"
            ))
        })
}
