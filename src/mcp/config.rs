use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

use crate::config;
use crate::durable::store;
use crate::error::{Error, Result};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub(crate) struct Entry {
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub url: Option<String>,
    #[serde(default, alias = "http_headers")]
    pub headers: BTreeMap<String, String>,
    pub bearer_token_env_var: Option<String>,
    pub enabled: Option<bool>,
    pub auth: Option<crate::config::OAuth>,
    pub tools: Option<Vec<String>>,
    pub idle_secs: Option<u64>,
    pub keep_alive: Option<bool>,
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub user_config: BTreeMap<String, String>,
}

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

impl Entry {
    fn overlay(&mut self, top: Self) {
        if top.command.is_none() && top.url.is_none() {
            self.enabled = top.enabled.or(self.enabled);
            self.tools = top.tools.or(self.tools.take());
            self.idle_secs = top.idle_secs.or(self.idle_secs);
            self.keep_alive = top.keep_alive.or(self.keep_alive);
            self.timeout_secs = top.timeout_secs.or(self.timeout_secs);
            self.env.extend(top.env);
            self.headers.extend(top.headers);
            self.user_config.extend(top.user_config);
            return;
        }
        let enabled = top.enabled.or(self.enabled);
        *self = top;
        self.enabled = enabled;
    }

    fn server(self, name: String, workspace: &Path) -> Result<Option<Server>> {
        for (key, value) in [
            ("timeout_secs", self.timeout_secs),
            ("idle_secs", self.idle_secs),
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
            crate::text::fnv64(
                &serde_json::to_vec(&(root, &name, &self.command, &self.args, &self.url))
                    .unwrap_or_default(),
            )
        );
        let allow = self.tools.clone().map(|tools| tools.into_iter().collect());
        let idle = (self.keep_alive != Some(true)).then(|| {
            self.idle_secs
                .map_or(crate::limits::limits().mcp_idle(), Duration::from_secs)
        });
        let wait = self
            .timeout_secs
            .map_or(crate::limits::limits().mcp_call(), Duration::from_secs);
        Ok(self.launch().map(|launch| Server {
            launch,
            name,
            key,
            allow,
            idle,
            wait,
        }))
    }

    fn launch(self) -> Option<Launch> {
        let user_config = self.user_config;
        let fill = |text: &str| expand(&substitute(text, &user_config));
        if let Some(url) = self.url {
            let mut headers: Vec<(String, String)> = self
                .headers
                .iter()
                .map(|(key, value)| (key.clone(), fill(value)))
                .collect();
            if let Some(var) = self.bearer_token_env_var
                && let Ok(token) = std::env::var(var)
            {
                headers.push(("Authorization".to_string(), format!("Bearer {token}")));
            }
            return Some(Launch::Http {
                url: fill(&url),
                headers,
                auth: self.auth.map(|auth| {
                    Box::new(crate::config::OAuth {
                        client_secret: auth.client_secret.as_deref().map(&fill),
                        refresh_token: auth.refresh_token.as_deref().map(&fill),
                        ..auth
                    })
                }),
            });
        }
        let command = self.command?;
        Some(Launch::Stdio {
            command: fill(&command),
            args: self.args.iter().map(|arg| fill(arg)).collect(),
            env: self
                .env
                .iter()
                .map(|(key, value)| (key.clone(), fill(value)))
                .collect(),
        })
    }
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
        if let Some(server) = entry.server(name, workspace)? {
            servers.push(server);
        }
    }
    Ok(Found { servers, waiting })
}

type Layers = (Vec<BTreeMap<String, Entry>>, Vec<String>);

fn layers(workspace: &Path) -> Result<Layers> {
    let home = config::home_dir();
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
        let codex = read_toml(&home.join(".codex/config.toml"))?;
        layers.push(toml_map(
            codex.as_ref().and_then(|file| file.get("mcp_servers")),
        ));
        let claude = read_json(&home.join(".claude.json"))?;
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
    let local = read_json(&workspace.join(".mcp.json"))?;
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
        let mut entries = BTreeMap::new();
        for map in plugin.mcp_json() {
            entries.extend(json_map(Some(&map)));
        }
        entries.extend(toml_map(plugin.mcp_toml().as_ref()));
        for (name, mut entry) in entries {
            entry.command = entry
                .command
                .map(|command| plugin.expand(&command, workspace));
            entry.args = entry
                .args
                .iter()
                .map(|arg| plugin.expand(arg, workspace))
                .collect();
            entry.env = entry
                .env
                .into_iter()
                .map(|(key, value)| (key, plugin.expand(&value, workspace)))
                .collect();
            entry.url = entry.url.map(|url| plugin.expand(&url, workspace));
            layer.insert(format!("{}-{name}", plugin.name), entry);
        }
    }
    layer
}

fn read_json(path: &Path) -> Result<Option<Value>> {
    let text = store::read_or_empty(path)?;
    if text.is_empty() {
        return Ok(None);
    }
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|error| Error::Config(format!("{}: {error}", path.display())))
}

fn read_toml(path: &Path) -> Result<Option<toml::Value>> {
    let text = store::read_or_empty(path)?;
    if text.is_empty() {
        return Ok(None);
    }
    toml::from_str(&text)
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

fn substitute(text: &str, config: &BTreeMap<String, String>) -> String {
    let mut out = String::new();
    let mut rest = text;
    let needle = "${user_config.";
    while let Some(start) = rest.find(needle) {
        let after = &rest[start + needle.len()..];
        let Some(end) = after.find('}') else {
            break;
        };
        out.push_str(&rest[..start]);
        match config.get(&after[..end]) {
            Some(value) => out.push_str(value),
            None => out.push_str(&rest[start..start + needle.len() + end + 1]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

fn expand(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        let Some(length) = rest[start..].find('}') else {
            break;
        };
        out.push_str(&rest[..start]);
        let inner = &rest[start + 2..start + length];
        let (name, fallback) = inner.split_once(":-").unwrap_or((inner, ""));
        out.push_str(&std::env::var(name).unwrap_or_else(|_unset| fallback.to_string()));
        rest = &rest[start + length + 1..];
    }
    out.push_str(rest);
    out
}
