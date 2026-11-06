use std::collections::BTreeMap;
use std::path::Path;

use super::{Plugin, data_dir};
use crate::mcp::scan;

const ROOTS: [&str; 2] = ["${CLAUDE_PLUGIN_ROOT}", "${SUMMONR_PLUGIN_ROOT}"];
const DATA: [&str; 2] = ["${CLAUDE_PLUGIN_DATA}", "${SUMMONR_PLUGIN_DATA}"];
const PROJECT: &str = "${CLAUDE_PROJECT_DIR}";
pub(crate) const USER_CONFIG: &str = "${user_config.";

#[derive(Clone, Copy, Default)]
pub(crate) struct Vars<'a> {
    pub(crate) plugin: Option<&'a Plugin>,
    pub(crate) workspace: Option<&'a Path>,
    pub(crate) user_config: Option<&'a BTreeMap<String, String>>,
}

pub(crate) fn expand(text: &str, vars: &Vars<'_>) -> String {
    let mut text = text.to_string();
    if let Some(plugin) = vars.plugin {
        let root = plugin.dir.to_string_lossy();
        text = ROOTS
            .iter()
            .fold(text, |text, token| text.replace(token, &root));
        if DATA.iter().any(|token| text.contains(token))
            && let Ok(data) = data_dir(&plugin.name)
        {
            drop(std::fs::create_dir_all(&data));
            let data = data.to_string_lossy();
            text = DATA
                .iter()
                .fold(text, |text, token| text.replace(token, &data));
        }
    }
    if let Some(workspace) = vars.workspace {
        text = text.replace(PROJECT, &workspace.to_string_lossy());
    }
    let Some(user_config) = vars.user_config else {
        return text;
    };
    let text = scan(&text, USER_CONFIG, |name| user_config.get(name).cloned());
    scan(&text, "${", |inner| {
        let (name, fallback) = inner.split_once(":-").unwrap_or((inner, ""));
        Some(std::env::var(name).unwrap_or_else(|_unset| fallback.to_string()))
    })
}
