use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use super::{Plugin, enabled, read_json};

const FILE: &str = ".lsp.json";

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
}

#[derive(Clone, Debug)]
pub(crate) struct Server {
    pub(crate) key: String,
    pub(crate) argv: Vec<String>,
    pub(crate) env: BTreeMap<String, String>,
    pub(crate) languages: BTreeMap<String, String>,
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
    let Ok(entries) = serde_json::from_value::<BTreeMap<String, serde_json::Value>>(read_json(
        &plugin.dir.join(FILE),
    )) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter_map(|(name, entry)| Some((name, serde_json::from_value::<Entry>(entry).ok()?)))
        .map(|(name, entry)| {
            let expand = |text: &String| plugin.expand(text, workspace);
            Server {
                key: format!("{}:{name}", plugin.name),
                argv: std::iter::once(&entry.command)
                    .chain(&entry.args)
                    .map(expand)
                    .collect(),
                env: entry
                    .env
                    .iter()
                    .map(|(key, value)| (key.clone(), expand(value)))
                    .collect(),
                languages: entry
                    .extension_to_language
                    .into_iter()
                    .map(|(extension, language)| {
                        (extension.trim_start_matches('.').to_string(), language)
                    })
                    .collect(),
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "lsp_tests.rs"]
mod tests;
