use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::error::{Error, Result};
use crate::hooks::{Dialect, Hook, HookEvent, Hooks, Matcher};
use crate::prompt::summonr_home;
use crate::skills;

pub(crate) mod agents;
pub(crate) mod commands;
pub(crate) mod lsp;
mod manage;

pub use manage::{Installed, check_servers, install, list, uninstall};

pub(super) const MANIFEST: &str = ".claude-plugin/plugin.json";
const ROOTS: [&str; 2] = ["${CLAUDE_PLUGIN_ROOT}", "${SUMMONR_PLUGIN_ROOT}"];
const DATA: [&str; 2] = ["${CLAUDE_PLUGIN_DATA}", "${SUMMONR_PLUGIN_DATA}"];
const PROJECT: &str = "${CLAUDE_PROJECT_DIR}";
pub(super) const USER_CONFIG: &str = "${user_config.";
pub(super) const UNSUPPORTED: [(&str, &str); 4] = [
    ("monitors", "monitors"),
    ("output-styles", "output styles"),
    ("workflows", "workflows"),
    ("themes", "themes"),
];

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct Switches {
    #[serde(default)]
    pub(crate) off: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct Plugin {
    pub(crate) name: String,
    pub(crate) dir: PathBuf,
    manifest: Value,
}

impl Plugin {
    pub(crate) fn at(name: String, dir: PathBuf) -> Self {
        let manifest = read_json(&dir.join(MANIFEST));
        Self {
            name,
            dir,
            manifest,
        }
    }

    fn inside(&self, relative: &str) -> Option<PathBuf> {
        let relative = relative.strip_prefix("./")?;
        let path = self.dir.join(relative);
        (!relative.split('/').any(|part| part == "..")).then_some(path)
    }

    fn paths(&self, key: &str) -> Vec<PathBuf> {
        match self.manifest.get(key) {
            Some(Value::String(one)) => self.inside(one).into_iter().collect(),
            Some(Value::Array(many)) => many
                .iter()
                .filter_map(Value::as_str)
                .filter_map(|one| self.inside(one))
                .collect(),
            Some(_) | None => Vec::new(),
        }
    }

    pub(super) fn declared(&self, key: &str) -> Vec<PathBuf> {
        self.paths(key)
    }

    fn hook_files(&self) -> Vec<PathBuf> {
        let mut files = vec![self.dir.join("hooks/hooks.json")];
        files.extend(self.paths("hooks"));
        let mut seen = BTreeSet::new();
        files.retain(|file| seen.insert(file.clone()));
        files
    }

    pub(super) fn skipped_mods(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for file in self.hook_files() {
            note_mods(&read_json(&file), &mut lines);
        }
        if let Some(inline) = self.manifest.get("hooks").filter(|value| value.is_object()) {
            note_mods(&serde_json::json!({ "hooks": inline }), &mut lines);
        }
        lines
    }

    pub(crate) fn skill_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.dir.join("skills")];
        dirs.extend(self.paths("skills"));
        dirs.into_iter().filter(|dir| dir.is_dir()).collect()
    }

    pub(crate) fn bin(&self) -> Option<PathBuf> {
        Some(self.dir.join("bin")).filter(|dir| dir.is_dir())
    }

    pub(crate) fn expand(&self, text: &str, workspace: &Path) -> String {
        let root = self.dir.to_string_lossy();
        let mut text = ROOTS
            .iter()
            .fold(text.to_string(), |text, token| text.replace(token, &root));
        if DATA.iter().any(|token| text.contains(token))
            && let Ok(data) = data_dir(&self.name)
        {
            drop(std::fs::create_dir_all(&data));
            let data = data.to_string_lossy();
            text = DATA
                .iter()
                .fold(text, |text, token| text.replace(token, &data));
        }
        text.replace(PROJECT, &workspace.to_string_lossy())
    }

    pub(crate) fn mcp_json(&self) -> Vec<Value> {
        let mut maps = vec![read_json(&self.dir.join(".mcp.json"))["mcpServers"].clone()];
        match self.manifest.get("mcpServers") {
            Some(Value::String(path)) => maps.extend(self.inside(path).map(|path| {
                let file = read_json(&path);
                file.get("mcpServers").cloned().unwrap_or(file)
            })),
            Some(inline @ Value::Object(_)) => maps.push(inline.clone()),
            Some(_) | None => {}
        }
        maps
    }

    pub(crate) fn mcp_toml(&self) -> Option<toml::Value> {
        let text = std::fs::read_to_string(self.dir.join("mcp.toml")).ok()?;
        toml::from_str::<toml::Value>(&text)
            .ok()?
            .get("mcp")
            .cloned()
    }

    pub(crate) fn hooks(&self) -> Hooks {
        let mut hooks = Hooks::default();
        for file in self.hook_files() {
            self.claude_hooks(&read_json(&file), &mut hooks);
        }
        if let Some(inline) = self.manifest.get("hooks").filter(|value| value.is_object()) {
            self.claude_hooks(&serde_json::json!({ "hooks": inline }), &mut hooks);
        }
        if let Ok(text) = std::fs::read_to_string(self.dir.join("hooks.toml"))
            && let Ok(native) = toml::from_str::<Hooks>(&text)
        {
            for event in HookEvent::ALL {
                for hook in native.of(event) {
                    hooks.push(
                        event,
                        Hook {
                            command: hook.command.clone(),
                            root: Some(self.dir.clone()),
                            ..hook.clone()
                        },
                    );
                }
            }
        }
        hooks
    }

    fn claude_hooks(&self, file: &Value, hooks: &mut Hooks) {
        for event in HookEvent::ALL {
            let groups = file["hooks"][event.claude()]
                .as_array()
                .cloned()
                .unwrap_or_default();
            for group in groups {
                let matcher = group["matcher"].as_str().and_then(Matcher::parse);
                for handler in group["hooks"].as_array().into_iter().flatten() {
                    if handler["type"] != "command" {
                        continue;
                    }
                    let Some(command) = handler["command"].as_str() else {
                        continue;
                    };
                    hooks.push(
                        event,
                        Hook {
                            command: command.to_string(),
                            dialect: Dialect::Claude,
                            matcher: matcher.clone(),
                            root: Some(self.dir.clone()),
                        },
                    );
                }
            }
        }
    }
}

fn note_mods(file: &Value, lines: &mut Vec<String>) {
    for event in HookEvent::ALL {
        let Some(groups) = file["hooks"][event.claude()].as_array() else {
            continue;
        };
        for group in groups {
            let Some(handlers) = group["hooks"].as_array() else {
                continue;
            };
            for handler in handlers {
                if handler["type"] == "command" {
                    continue;
                }
                let kind = handler["type"].as_str().unwrap_or("hook");
                lines.push(format!(
                    "skipped\t{} {kind} (summonr runs command hooks only)",
                    event.claude()
                ));
            }
        }
    }
}

pub(super) fn read_json(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

pub(super) fn json_file(path: &Path) -> Result<Value> {
    let Some(text) = read_text(path)? else {
        return Ok(Value::Null);
    };
    serde_json::from_str(&text)
        .map_err(|error| Error::Config(format!("{}: {error}", path.display())))
}

pub(super) fn require_tree(plugin: &Plugin) -> Result<()> {
    for path in [
        plugin.dir.join(".mcp.json"),
        plugin.dir.join("hooks/hooks.json"),
        plugin.dir.join(".lsp.json"),
    ] {
        json_file(&path)?;
    }
    parsed(&plugin.dir.join("mcp.toml"), |text| {
        toml::from_str::<toml::Value>(text)
            .map(|_| ())
            .map_err(|error| error.to_string())
    })?;
    parsed(&plugin.dir.join("hooks.toml"), |text| {
        toml::from_str::<Hooks>(text)
            .map(|_| ())
            .map_err(|error| error.to_string())
    })?;
    for path in plugin.declared("mcpServers") {
        json_file(&path)?;
    }
    for path in plugin.declared("hooks") {
        json_file(&path)?;
    }
    Ok(())
}

fn parsed(path: &Path, parse: impl FnOnce(&str) -> std::result::Result<(), String>) -> Result<()> {
    let Some(text) = read_text(path)? else {
        return Ok(());
    };
    parse(&text).map_err(|error| Error::Config(format!("{}: {error}", path.display())))
}

fn read_text(path: &Path) -> Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(None),
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::Config(format!("{}: {error}", path.display()))),
    }
}

pub(super) fn root() -> Result<PathBuf> {
    Ok(summonr_home()?.join("plugins"))
}

pub(crate) fn data_dir(name: &str) -> Result<PathBuf> {
    Ok(summonr_home()?.join("plugin-data").join(name))
}

pub(super) fn installed() -> Vec<Plugin> {
    let Ok(root) = root() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut plugins: Vec<Plugin> = entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|dir| {
            let name = dir.file_name()?.to_str()?.to_string();
            skills::valid(&name).then(|| Plugin::at(name, dir))
        })
        .collect();
    plugins.sort_by(|left, right| left.name.cmp(&right.name));
    plugins
}

pub(crate) fn enabled() -> Vec<Plugin> {
    let file = crate::config::file().ok();
    let off = file
        .as_ref()
        .map_or(&[][..], |file| file.plugins.off.as_slice());
    installed()
        .into_iter()
        .filter(|plugin| !off.contains(&plugin.name))
        .collect()
}

static WARNED_OFF: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn unknown_off(off: &[String]) -> Option<String> {
    if off.is_empty() {
        return None;
    }
    let installed = installed();
    let known: BTreeSet<&str> = installed
        .iter()
        .map(|plugin| plugin.name.as_str())
        .collect();
    let unknown: Vec<&str> = off
        .iter()
        .filter(|name| !known.contains(name.as_str()))
        .map(String::as_str)
        .collect();
    (!unknown.is_empty()).then(|| {
        format!(
            "[plugins] off names plugins that are not installed: {}; `summonr uninstall NAME` removes the name",
            unknown.join(", ")
        )
    })
}

pub(crate) fn warn_unknown_off(off: &[String]) {
    if let Some(note) = unknown_off(off)
        && !WARNED_OFF.swap(true, std::sync::atomic::Ordering::Relaxed)
    {
        eprintln!("summonr: {note}");
    }
}

pub(crate) fn bin_path_prefix() -> Option<String> {
    let mut dirs: Vec<PathBuf> = summonr_home()
        .ok()
        .and_then(|home| std::fs::read_dir(home.join("bin")).ok())
        .into_iter()
        .flatten()
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    dirs.extend(enabled().iter().filter_map(Plugin::bin));
    if dirs.is_empty() {
        return None;
    }
    let sep = if cfg!(windows) { ";" } else { ":" };
    Some(
        dirs.iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join(sep),
    )
}

#[cfg(test)]
#[path = "plugins_tests.rs"]
mod tests;
