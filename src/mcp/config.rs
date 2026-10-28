use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::config;
use crate::durable::store;
use summonr_core::error::{Error, Result};

pub(crate) use summonr_types::mcp::Entry;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Launch {
    Stdio {
        command: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
    Http {
        url: String,
        headers: Vec<(String, String)>,
        auth: Option<Box<crate::config::OAuth>>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Server {
    pub name: String,
    pub launch: Launch,
    pub key: String,
    pub allow: Option<std::collections::BTreeSet<String>>,
    pub idle: Option<Duration>,
    pub wait: Duration,
}

fn server(entry: Entry, name: String, workspace: &Path) -> Result<Option<Server>> {
    for (key, value) in [
        ("timeout_secs", entry.timeout_secs),
        ("idle_secs", entry.idle_secs),
    ] {
        if value == Some(0) {
            return Err(Error::Config(format!(
                "mcp {name} {key} must be at least 1"
            )));
        }
    }
    let root = dunce::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    let key = format!(
        "{}-{}",
        super::catalog::stem(&name),
        summonr_core::text::fnv64(
            &serde_json::to_vec(&(root, &name, &entry.command, &entry.args, &entry.url))
                .unwrap_or_default(),
        )
    );
    let allow = entry.tools.clone().map(|tools| tools.into_iter().collect());
    let idle = (entry.keep_alive != Some(true)).then(|| {
        entry
            .idle_secs
            .map_or(crate::limits::limits().mcp_idle(), Duration::from_secs)
    });
    let wait = entry
        .timeout_secs
        .map_or(crate::limits::limits().mcp_call(), Duration::from_secs);
    Ok(launch(entry).map(|launch| Server {
        launch,
        name,
        key,
        allow,
        idle,
        wait,
    }))
}

fn launch(entry: Entry) -> Option<Launch> {
    let user_config = entry.user_config;
    let fill = |text: &str| expand_vars(text, &user_config);
    if let Some(url) = entry.url {
        let mut headers: Vec<(String, String)> = entry
            .headers
            .iter()
            .map(|(key, value)| (key.clone(), fill(value)))
            .collect();
        if let Some(var) = entry.bearer_token_env_var
            && let Ok(token) = std::env::var(var)
        {
            headers.push(("Authorization".to_string(), format!("Bearer {token}")));
        }
        return Some(Launch::Http {
            url: fill(&url),
            headers,
            auth: entry.auth.map(|auth| {
                Box::new(crate::config::OAuth {
                    client_secret: auth.client_secret.as_deref().map(&fill),
                    refresh_token: auth.refresh_token.as_deref().map(&fill),
                    ..auth
                })
            }),
        });
    }
    let command = entry.command?;
    Some(Launch::Stdio {
        command: fill(&command),
        args: entry.args.iter().map(|arg| fill(arg)).collect(),
        env: entry
            .env
            .iter()
            .map(|(key, value)| (key.clone(), fill(value)))
            .collect(),
    })
}

pub(crate) struct Found {
    pub servers: Vec<Server>,
    pub waiting: Vec<String>,
}

pub(crate) fn servers(workspace: &Path) -> Result<Found> {
    let (layers, waiting) = layers(workspace)?;
    let mut entries: BTreeMap<String, Entry> = BTreeMap::new();
    for layer in layers {
        for (name, entry) in layer {
            match entries.get_mut(&name) {
                Some(base) => base.overlay(entry),
                None => {
                    entries.insert(name, entry);
                }
            }
        }
    }
    let mut servers = Vec::new();
    for (name, entry) in entries {
        if entry.enabled == Some(false) {
            continue;
        }
        if let Some(server) = server(entry, name, workspace)? {
            servers.push(server);
        }
    }
    Ok(Found { servers, waiting })
}

type Layers = (Vec<BTreeMap<String, Entry>>, Vec<String>);

fn layers(workspace: &Path) -> Result<Layers> {
    let home = summonr_core::dirs::home_dir();
    let mut layers = Vec::new();
    let mut approved = BTreeSet::new();
    let mut all = false;
    let file = config::file()?;
    let user = file.mcp.clone();
    let import_peers = file.import_mcp == Some(true);
    approved.extend(
        user.iter()
            .filter(|(_, entry)| entry.enabled == Some(true))
            .map(|(name, _)| name.clone()),
    );
    if import_peers && let Some(home) = &home {
        let codex = read(&home.join(".codex/config.toml"), |text: &str| {
            toml::from_str::<toml::Value>(text)
        })?;
        layers.push(toml_map(
            codex.as_ref().and_then(|file| file.get("mcp_servers")),
        ));
        let claude = read(&home.join(".claude.json"), |text: &str| {
            serde_json::from_str::<Value>(text)
        })?;
        layers.push(json_map(
            claude.as_ref().and_then(|file| file.get("mcpServers")),
        ));
        let project = claude
            .as_ref()
            .and_then(|file| file.get("projects")?.get(workspace.to_str()?));
        layers.push(json_map(project.and_then(|entry| entry.get("mcpServers"))));
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
    }
    let local = read(&workspace.join(".mcp.json"), |text: &str| {
        serde_json::from_str::<Value>(text)
    })?;
    let mut shared = json_map(local.as_ref().and_then(|file| file.get("mcpServers")));
    let path = workspace.join(".summonr/config.toml");
    if path.is_file() {
        shared.extend(config::parse(&path)?.mcp);
    }
    let waiting = shared
        .keys()
        .filter(|name| {
            !all && !approved.contains(*name)
                && !user
                    .get(*name)
                    .is_some_and(|entry| entry.command.is_some() || entry.url.is_some())
        })
        .cloned()
        .collect();
    shared.retain(|name, _entry| all || approved.contains(name));
    layers.push(shared);
    layers.push(plugin_layer(workspace));
    layers.push(user);
    Ok((layers, waiting))
}

fn plugin_layer(workspace: &Path) -> BTreeMap<String, Entry> {
    let mut layer = BTreeMap::new();
    for plugin in crate::plugins::enabled() {
        let mut maps = plugin.mcp_json();
        let toml = plugin.mcp_toml().map(serde_json::to_value);
        maps.extend(toml.into_iter().flatten());
        layer.extend(plugin.servers(&maps, "-", workspace, |entry: &mut Entry| {
            entry
                .command
                .iter_mut()
                .chain(&mut entry.url)
                .chain(&mut entry.args)
                .chain(entry.env.values_mut())
                .collect()
        }));
    }
    layer
}

fn read<T, E: std::fmt::Display>(
    path: &Path,
    parse: impl FnOnce(&str) -> std::result::Result<T, E>,
) -> Result<Option<T>> {
    let text = store::read_or_empty(path)?;
    if text.is_empty() {
        return Ok(None);
    }
    parse(&text)
        .map(Some)
        .map_err(|error| Error::Config(format!("{}: {error}", path.display())))
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

pub(crate) fn expand_vars(text: &str, user_config: &BTreeMap<String, String>) -> String {
    let text = scan(text, crate::plugins::USER_CONFIG, |name| {
        user_config.get(name).cloned()
    });
    scan(&text, "${", |inner| {
        let (name, fallback) = inner.split_once(":-").unwrap_or((inner, ""));
        Some(std::env::var(name).unwrap_or_else(|_unset| fallback.to_string()))
    })
}

pub(crate) fn scan(text: &str, open: &str, resolve: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find(open) {
        let after = &rest[start + open.len()..];
        let Some(end) = after.find('}') else {
            break;
        };
        out.push_str(&rest[..start]);
        match resolve(&after[..end]) {
            Some(value) => out.push_str(&value),
            None => out.push_str(&rest[start..start + open.len() + end + 1]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}
