use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;
use crate::hooks::{Dialect, Hook, HookEvent, Hooks, Matcher};
use crate::prompt::summonr_home;
use crate::skills;

pub(crate) mod agents;
pub(crate) mod commands;
pub(crate) mod lsp;
mod manage;

pub use manage::{install, list, uninstall};

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
        let mut files = vec![self.dir.join("hooks/hooks.json")];
        files.extend(self.paths("hooks"));
        for file in files {
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

pub(super) fn read_json(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
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
