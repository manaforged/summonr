use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::config::Entry;
use crate::config;
use crate::plugins::{expand, read_parsed, renamed, server_entry};
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
    pub skipped: Vec<(String, String)>,
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
    let mut skipped = Vec::new();
    if file.import_mcp == Some(true)
        && let Some(home) = &home
    {
        let claude_path = home.join(".claude.json");
        let claude = read_parsed(&claude_path, |text: &str| {
            serde_json::from_str::<Value>(text)
        })?;
        let project = claude
            .as_ref()
            .and_then(|file| file.get("projects")?.get(workspace.to_str()?));
        local = json_map(
            project.and_then(|entry| entry.get("mcpServers")),
            &claude_path,
            &mut skipped,
        );
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
            json_map(
                claude.as_ref().and_then(|file| file.get("mcpServers")),
                &claude_path,
                &mut skipped,
            ),
        ));
        let codex_path = home.join(".codex/config.toml");
        let codex = read_parsed(&codex_path, |text: &str| {
            toml::from_str::<toml::Value>(text)
        })?;
        let codex = codex
            .as_ref()
            .and_then(|file| file.get("mcp_servers")?.as_table());
        peers.push((
            at(Scope::User, &home.join(".codex")),
            read_entries(codex.into_iter().flatten(), &codex_path, &mut skipped),
        ));
    }
    let shared_path = workspace.join(".mcp.json");
    let shared_file = read_parsed(&shared_path, |text: &str| {
        serde_json::from_str::<Value>(text)
    })?;
    let mut shared = json_map(
        shared_file.as_ref().and_then(|file| file.get("mcpServers")),
        &shared_path,
        &mut skipped,
    );
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
    let legacy = plugins(workspace, &mut entries, &mut skipped);
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
    Ok(Resolved {
        entries,
        waiting,
        skipped,
    })
}

fn plugins(
    workspace: &Path,
    entries: &mut BTreeMap<String, Sourced>,
    skipped: &mut Vec<(String, String)>,
) -> BTreeMap<String, String> {
    let mut taken: BTreeSet<String> = entries
        .values()
        .filter(|found| found.entry.enabled != Some(false))
        .filter_map(|found| found.entry.endpoint())
        .collect();
    let mut legacy = BTreeMap::new();
    for plugin in crate::plugins::enabled() {
        let servers = plugin.mcp_servers();
        legacy.extend(plugin.legacy_names(&servers));
        let vars = plugin.vars(workspace);
        for server in servers {
            let entry: Entry = match server_entry(&server.file, &server.key, server.config) {
                Ok(entry) => entry,
                Err(reason) => {
                    skipped.push((server.name, reason));
                    continue;
                }
            };
            if entry
                .endpoint()
                .is_some_and(|endpoint| !taken.insert(expand(&endpoint, &vars)))
            {
                continue;
            }
            entries.entry(server.name).or_insert_with(|| Sourced {
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

fn json_map(
    value: Option<&Value>,
    file: &Path,
    skipped: &mut Vec<(String, String)>,
) -> BTreeMap<String, Entry> {
    let table = value.and_then(Value::as_object).into_iter().flatten();
    read_entries(table, file, skipped)
}

fn read_entries<'a, D>(
    table: impl Iterator<Item = (&'a String, &'a D)>,
    file: &Path,
    skipped: &mut Vec<(String, String)>,
) -> BTreeMap<String, Entry>
where
    D: Clone + for<'de> serde::Deserializer<'de> + 'a,
{
    let mut entries = BTreeMap::new();
    for (name, entry) in table {
        match server_entry(file, name, entry.clone()) {
            Ok(entry) => {
                entries.insert(name.clone(), entry);
            }
            Err(reason) => skipped.push((name.clone(), reason)),
        }
    }
    entries
}
