use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use super::{Plugin, data_dir};
use crate::mcp::scan;

const ROOTS: [&str; 2] = ["CLAUDE_PLUGIN_ROOT", "SUMMONR_PLUGIN_ROOT"];
const DATA: [&str; 2] = ["CLAUDE_PLUGIN_DATA", "SUMMONR_PLUGIN_DATA"];
const PROJECT: &str = "CLAUDE_PROJECT_DIR";
const SKILL_DIR: &str = "CLAUDE_SKILL_DIR";
const SESSION: &str = "CLAUDE_SESSION_ID";
pub(crate) const USER_CONFIG: &str = "${user_config.";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Unset {
    #[default]
    Empty,
    Kept,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct Vars<'a> {
    pub(crate) plugin: Option<&'a Plugin>,
    pub(crate) workspace: Option<&'a Path>,
    pub(crate) user_config: Option<&'a BTreeMap<String, String>>,
    pub(crate) unset: Unset,
}

fn placeholder(name: &str) -> String {
    format!("${{{name}}}")
}

fn fill(text: String, names: &[&str], value: &str) -> String {
    names
        .iter()
        .fold(text, |text, name| text.replace(&placeholder(name), value))
}

pub(crate) fn expand(text: &str, vars: &Vars<'_>) -> String {
    let mut text = text.to_string();
    if let Some(plugin) = vars.plugin {
        text = fill(text, &ROOTS, &plugin.dir.to_string_lossy());
        if DATA.iter().any(|name| text.contains(&placeholder(name)))
            && let Ok(data) = data_dir(&plugin.name)
        {
            drop(std::fs::create_dir_all(&data));
            text = fill(text, &DATA, &data.to_string_lossy());
        }
    }
    if let Some(workspace) = vars.workspace {
        text = fill(text, &[PROJECT], &workspace.to_string_lossy());
    }
    let Some(user_config) = vars.user_config else {
        return text;
    };
    let text = scan(&text, USER_CONFIG, |name| {
        Some(user_config.get(name).cloned().unwrap_or_default())
    });
    scan(&text, "${", |inner| {
        let (name, fallback) = inner
            .split_once(":-")
            .map_or((inner, None), |(name, fallback)| (name, Some(fallback)));
        std::env::var(name)
            .ok()
            .or_else(|| fallback.map(str::to_string))
            .or_else(|| match vars.unset {
                Unset::Empty => Some(String::new()),
                Unset::Kept => None,
            })
    })
}

/// Skill or command `text` with the plugin, project, skill folder, and session placeholders filled.
pub(crate) fn content(
    text: &str,
    plugin: Option<&Plugin>,
    skill: Option<&Path>,
    workspace: &Path,
    session: Option<&str>,
) -> String {
    let vars = Vars {
        plugin,
        workspace: Some(workspace),
        ..Vars::default()
    };
    let mut text = expand(text, &vars);
    if let Some(dir) = skill {
        text = fill(text, &[SKILL_DIR], &dir.to_string_lossy());
    }
    match session {
        Some(id) => fill(text, &[SESSION], id),
        None => text,
    }
}

/// The variables a hook process gets: the project folder, and a plugin hook's root and data folders.
pub(crate) fn hook_env(workspace: &Path, root: Option<&Path>) -> Vec<(&'static str, OsString)> {
    let data = root
        .and_then(Path::file_name)
        .and_then(|name| data_dir(&name.to_string_lossy()).ok());
    let mut env = vec![(PROJECT, workspace.as_os_str().to_owned())];
    let plugin = [
        (ROOTS, root.map(|root| root.as_os_str().to_owned())),
        (DATA, data.map(std::path::PathBuf::into_os_string)),
    ];
    for (names, value) in plugin {
        if let Some(value) = value {
            env.extend(names.map(|name| (name, value.clone())));
        }
    }
    env
}
