use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::config;

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
}

impl Entry {
    fn overlay(&mut self, top: Self) {
        if top.command.is_none() && top.url.is_none() {
            self.enabled = top.enabled.or(self.enabled);
            return;
        }
        let enabled = top.enabled.or(self.enabled);
        *self = top;
        self.enabled = enabled;
    }

    fn launch(self) -> Option<Launch> {
        if let Some(url) = self.url {
            let mut headers: Vec<(String, String)> = self
                .headers
                .into_iter()
                .map(|(key, value)| (key, expand(&value)))
                .collect();
            if let Some(var) = self.bearer_token_env_var
                && let Ok(token) = std::env::var(var)
            {
                headers.push(("Authorization".to_string(), format!("Bearer {token}")));
            }
            return Some(Launch::Http {
                url: expand(&url),
                headers,
                auth: self.auth.map(|auth| {
                    Box::new(crate::config::OAuth {
                        client_secret: auth.client_secret.as_deref().map(expand),
                        refresh_token: auth.refresh_token.as_deref().map(expand),
                        ..auth
                    })
                }),
            });
        }
        Some(Launch::Stdio {
            command: expand(&self.command?),
            args: self.args.iter().map(|arg| expand(arg)).collect(),
            env: self
                .env
                .into_iter()
                .map(|(key, value)| (key, expand(&value)))
                .collect(),
        })
    }
}

pub(crate) fn servers(workspace: &Path) -> Vec<Server> {
    let mut entries: BTreeMap<String, Entry> = BTreeMap::new();
    for layer in layers(workspace) {
        for (name, entry) in layer {
            match entries.get_mut(&name) {
                Some(base) => base.overlay(entry),
                None => {
                    entries.insert(name, entry);
                }
            }
        }
    }
    entries
        .into_iter()
        .filter(|(_, entry)| entry.enabled != Some(false))
        .filter_map(|(name, entry)| {
            Some(Server {
                launch: entry.launch()?,
                name,
            })
        })
        .filter(|server| !ours(server))
        .collect()
}

fn layers(workspace: &Path) -> Vec<BTreeMap<String, Entry>> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut layers = Vec::new();
    let mut approved = BTreeSet::new();
    let mut all = false;
    let file = config::load().ok();
    let user = file
        .as_ref()
        .map(|file| file.mcp.clone())
        .unwrap_or_default();
    let import_peers = file.as_ref().and_then(|file| file.import_mcp) == Some(true);
    approved.extend(
        user.iter()
            .filter(|(_, entry)| entry.enabled == Some(true))
            .map(|(name, _)| name.clone()),
    );
    if import_peers && let Some(home) = &home {
        let codex = read_toml(&home.join(".codex/config.toml"));
        layers.push(toml_map(
            codex.as_ref().and_then(|file| file.get("mcp_servers")),
        ));
        let claude = read_json(&home.join(".claude.json"));
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
    let local = read_json(&workspace.join(".mcp.json"));
    let mut shared = json_map(local.as_ref().and_then(|file| file.get("mcpServers")));
    let path = workspace.join(".summonr/config.toml");
    if path.is_file()
        && let Ok(file) = config::load_from(&path)
    {
        shared.extend(file.mcp);
    }
    shared.retain(|name, _entry| all || approved.contains(name));
    layers.push(shared);
    layers.push(user);
    layers
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

fn read_toml(path: &Path) -> Option<toml::Value> {
    toml::from_str(&std::fs::read_to_string(path).ok()?).ok()
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

fn ours(server: &Server) -> bool {
    let Launch::Stdio { command, args, .. } = &server.launch else {
        return false;
    };
    let program = Path::new(command)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    matches!(program, "summon" | "summonr") && args.first().is_some_and(|arg| arg == "room")
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
