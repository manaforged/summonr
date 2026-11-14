use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use super::{LSP, Plugin, Unset, Vars, enabled, expand, server_entry};

const USED: [&str; 7] = [
    "command",
    "args",
    "extensionToLanguage",
    "env",
    "initializationOptions",
    "workspaceFolder",
    "transport",
];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    extension_to_language: BTreeMap<String, String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    initialization_options: Option<Value>,
    workspace_folder: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Server {
    pub(crate) key: String,
    pub(crate) argv: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) languages: BTreeMap<String, String>,
    pub(crate) options: Value,
    pub(crate) root: Option<PathBuf>,
}

impl Server {
    pub(crate) fn handles(&self, path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| self.languages.contains_key(extension))
    }
}

pub(crate) fn servers(workspace: &Path) -> Vec<Server> {
    enabled()
        .iter()
        .flat_map(|plugin| of(plugin, workspace))
        .collect()
}

pub(super) fn of(plugin: &Plugin, workspace: &Path) -> Vec<Server> {
    let user_config = BTreeMap::new();
    let vars = Vars {
        user_config: Some(&user_config),
        env: Some(Unset::Empty),
        ..plugin.vars(workspace)
    };
    let fill = |text: String| expand(&text, &vars);
    entries(plugin)
        .flat_map(|(key, entry)| entry.map(|entry| (key, entry)))
        .map(|(key, entry)| Server {
            key,
            argv: std::iter::once(entry.command)
                .chain(entry.args)
                .map(fill)
                .collect(),
            env: entry
                .env
                .into_iter()
                .map(|(key, value)| (key, fill(value)))
                .collect(),
            languages: entry
                .extension_to_language
                .into_iter()
                .map(|(extension, language)| {
                    (extension.trim_start_matches('.').to_string(), language)
                })
                .collect(),
            options: entry
                .initialization_options
                .unwrap_or_else(|| serde_json::json!({})),
            root: entry
                .workspace_folder
                .map(|folder| workspace.join(fill(folder))),
        })
        .collect()
}

fn entries(plugin: &Plugin) -> impl Iterator<Item = (String, Result<Entry, String>)> {
    plugin
        .merged(LSP)
        .into_iter()
        .map(|(server, (file, entry))| {
            (plugin.scoped(&server), server_entry(&file, &server, entry))
        })
}

/// Refuses a language server entry `of` would drop, naming its file and key.
pub(super) fn check(plugin: &Plugin) -> summonr_core::error::Result<()> {
    for (_, entry) in entries(plugin) {
        entry.map_err(summonr_core::error::Error::Config)?;
    }
    Ok(())
}

/// Install-report lines for the language server fields summonr does not apply.
pub(super) fn unused(plugin: &Plugin) -> Vec<String> {
    plugin
        .merged(LSP)
        .iter()
        .flat_map(|(server, (_, entry))| {
            entry
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(field, _)| !USED.contains(&field.as_str()))
                .map(move |(field, _)| {
                    format!("skipped\tlsp {server} {field} (summonr does not apply it)")
                })
        })
        .collect()
}

#[cfg(test)]
#[path = "lsp_tests.rs"]
mod tests;
