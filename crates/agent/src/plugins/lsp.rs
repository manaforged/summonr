use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use super::{Plugin, Vars, enabled, expand, read_json};

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
    let maps = [read_json(&plugin.dir.join(FILE))];
    let user_config = BTreeMap::new();
    let vars = Vars {
        user_config: Some(&user_config),
        ..plugin.vars(workspace)
    };
    let fill = |text: String| expand(&text, &vars);
    plugin
        .servers(&maps, ":", workspace, |_entry: &mut Entry| Vec::new())
        .into_iter()
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
        })
        .collect()
}

#[cfg(test)]
#[path = "lsp_tests.rs"]
mod tests;
