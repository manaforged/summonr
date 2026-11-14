use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::config::Entry;
use crate::config;
use crate::plugins::read_parsed;
use summonr_core::error::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    Local,
    Project,
    User,
    Plugin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Origin {
    pub scope: Scope,
    pub dir: PathBuf,
}

pub(super) struct Sourced {
    pub entry: Entry,
    pub origin: Origin,
}

pub(super) struct Resolved {
    pub entries: BTreeMap<String, Sourced>,
    pub waiting: Vec<String>,
}

type Layer = (Origin, BTreeMap<String, Entry>);

pub(super) fn resolve(workspace: &Path) -> Result<Resolved> {
    let home = summonr_core::dirs::home_dir();
    let own = summonr_core::dirs::summonr_home().unwrap_or_else(|_| workspace.to_path_buf());
    let file = config::file()?;
    let user = file.mcp.clone();
    let mut approved: BTreeSet<String> = user
        .iter()
        .filter(|(_, entry)| entry.enabled == Some(true))
        .map(|(name, _)| name.clone())
        .collect();
    let at = |scope: Scope, dir: &Path| Origin {
        scope,
        dir: dir.to_path_buf(),
    };
    let mut peers: Vec<Layer> = Vec::new();
    let mut local = BTreeMap::new();
    let mut all = false;
    if file.import_mcp == Some(true)
        && let Some(home) = &home
    {
        let claude = read_parsed(&home.join(".claude.json"), |text: &str| {
            serde_json::from_str::<Value>(text)
        })?;
        let project = claude
            .as_ref()
            .and_then(|file| file.get("projects")?.get(workspace.to_str()?));
        local = json_map(project.and_then(|entry| entry.get("mcpServers")));
        all = project
            .and_then(|entry| entry.get("enableAllProjectMcpServers"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        approved.extend(
            project
                .and_then(|entry| entry.get("enabledMcpjsonServers"))
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_string),
        );
        peers.push((
            at(Scope::User, &summonr_core::dirs::claude(home)),
            json_map(claude.as_ref().and_then(|file| file.get("mcpServers"))),
        ));
        let codex = read_parsed(&home.join(".codex/config.toml"), |text: &str| {
            toml::from_str::<toml::Value>(text)
        })?;
        peers.push((
            at(Scope::User, &home.join(".codex")),
            toml_map(codex.as_ref().and_then(|file| file.get("mcp_servers"))),
        ));
    }
    let shared_file = read_parsed(&workspace.join(".mcp.json"), |text: &str| {
        serde_json::from_str::<Value>(text)
    })?;
    let mut shared = json_map(shared_file.as_ref().and_then(|file| file.get("mcpServers")));
    shared.extend(config::workspace(workspace)?.mcp.clone());
    let waiting = shared
        .keys()
        .filter(|name| {
            !all && !approved.contains(*name)
                && !local.contains_key(*name)
                && !user
                    .get(*name)
                    .is_some_and(|entry| entry.command.is_some() || entry.url.is_some())
        })
        .cloned()
        .collect();
    shared.retain(|name, _entry| all || approved.contains(name));
    let mut ranked = vec![
        (at(Scope::Local, workspace), local),
        (at(Scope::Project, workspace), shared),
    ];
    ranked.extend(peers);
    let mut entries = BTreeMap::new();
    for (origin, layer) in ranked {
        for (name, entry) in layer {
            entries.entry(name).or_insert_with(|| Sourced {
                entry,
                origin: origin.clone(),
            });
        }
    }
    let legacy = plugins(workspace, &mut entries);
    for (name, entry) in renamed(user, &legacy) {
        let launches = entry.command.is_some() || entry.url.is_some();
        match entries.get_mut(&name) {
            Some(base) => {
                base.entry.overlay(entry);
                if launches {
                    base.origin = at(Scope::User, &own);
                }
            }
            None => {
                entries.insert(
                    name,
                    Sourced {
                        entry,
                        origin: at(Scope::User, &own),
                    },
                );
            }
        }
    }
    Ok(Resolved { entries, waiting })
}

fn plugins(workspace: &Path, entries: &mut BTreeMap<String, Sourced>) -> BTreeMap<String, String> {
    let mut taken: BTreeSet<String> = entries
        .values()
        .filter(|found| found.entry.enabled != Some(false))
        .filter_map(|found| found.entry.endpoint())
        .collect();
    let mut legacy = BTreeMap::new();
    for plugin in crate::plugins::enabled() {
        let servers = plugin.mcp_servers();
        legacy.extend(
            servers
                .iter()
                .map(|server| (plugin.legacy_mcp_name(&server.key), server.name.clone())),
        );
        let named = servers
            .into_iter()
            .map(|server| (server.name, server.config));
        let found = plugin.servers(named, workspace, |entry: &mut Entry| {
            entry
                .command
                .iter_mut()
                .chain(&mut entry.url)
                .chain(&mut entry.headers_helper)
                .chain(&mut entry.args)
                .chain(entry.env.values_mut())
                .chain(entry.headers.values_mut())
                .collect()
        });
        for (name, entry) in found {
            if entry
                .endpoint()
                .is_some_and(|endpoint| !taken.insert(endpoint))
            {
                continue;
            }
            entries.entry(name).or_insert_with(|| Sourced {
                entry,
                origin: Origin {
                    scope: Scope::Plugin,
                    dir: plugin.dir.clone(),
                },
            });
        }
    }
    legacy
}

fn renamed(
    layer: BTreeMap<String, Entry>,
    legacy: &BTreeMap<String, String>,
) -> BTreeMap<String, Entry> {
    let mut out: BTreeMap<String, Entry> = BTreeMap::new();
    for (name, entry) in layer {
        let name = legacy.get(&name).cloned().unwrap_or(name);
        match out.get_mut(&name) {
            Some(base) => base.overlay(entry),
            None => {
                out.insert(name, entry);
            }
        }
    }
    out
}

fn json_map(value: Option<&Value>) -> BTreeMap<String, Entry> {
    value
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(name, entry)| {
            Some((name.clone(), serde_json::from_value(entry.clone()).ok()?))
        })
        .collect()
}

fn toml_map(value: Option<&toml::Value>) -> BTreeMap<String, Entry> {
    value
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
        .filter_map(|(name, entry)| Some((name.clone(), entry.clone().try_into().ok()?)))
        .collect()
}
