use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::hooks::{Hook, HookEvent, Hooks};
use crate::skills;
use summonr_core::dirs::summonr_home;
use summonr_core::error::{Error, Result};

pub(crate) mod agents;
pub(crate) mod commands;
mod expand;
mod hooks;
pub(crate) mod lsp;
pub mod manage;

pub(crate) use expand::{USER_CONFIG, Vars, expand};
pub(crate) use manage::off::{unknown_off, warn_unknown_off};

pub(super) const MANIFEST: &str = ".claude-plugin/plugin.json";
pub(super) const HOOKS: &str = "hooks";
pub(super) const UNSUPPORTED: [(&str, &str); 4] = [
    ("monitors", "monitors"),
    ("output-styles", "output styles"),
    ("workflows", "workflows"),
    ("themes", "themes"),
];
pub(super) const MCP: Merge = Merge {
    file: ".mcp.json",
    key: "mcpServers",
    wrapped: true,
};
pub(super) const LSP: Merge = Merge {
    file: ".lsp.json",
    key: "lspServers",
    wrapped: false,
};

/// A server map a plugin keeps in a default file and may extend from its manifest.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Merge {
    pub(crate) file: &'static str,
    pub(crate) key: &'static str,
    wrapped: bool,
}

/// One value a manifest component key declares.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Source {
    File(PathBuf),
    Inline(Value),
    Outside(String),
}

#[derive(Clone, Debug)]
pub struct Plugin {
    pub name: String,
    pub dir: PathBuf,
    pub manifest: Value,
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
        let relative = relative.strip_prefix("./").unwrap_or(relative);
        let relative = if relative == "." { "" } else { relative };
        let escapes = Path::new(relative).is_absolute()
            || relative.split(['/', '\\']).any(|part| part == "..");
        (!escapes).then(|| {
            if relative.is_empty() {
                self.dir.clone()
            } else {
                self.dir.join(relative)
            }
        })
    }

    /// Each value the manifest declares under `key`, or `None` when the key is absent.
    pub(crate) fn declared(&self, key: &str) -> Option<Vec<Source>> {
        let value = self.manifest.get(key)?;
        let items = match value {
            Value::Array(many) => many.as_slice(),
            one => std::slice::from_ref(one),
        };
        Some(
            items
                .iter()
                .map(|item| match item {
                    Value::String(path) => self
                        .inside(path)
                        .map_or_else(|| Source::Outside(path.clone()), Source::File),
                    other => Source::Inline(other.clone()),
                })
                .collect(),
        )
    }

    pub(crate) fn files(&self, key: &str) -> Vec<PathBuf> {
        self.declared(key)
            .into_iter()
            .flatten()
            .filter_map(|source| match source {
                Source::File(path) => Some(path),
                Source::Inline(_) | Source::Outside(_) => None,
            })
            .collect()
    }

    fn hook_docs(&self) -> Vec<Value> {
        let mut files = vec![self.dir.join("hooks/hooks.json")];
        let mut inline = Vec::new();
        for source in self.declared(HOOKS).into_iter().flatten() {
            match source {
                Source::File(file) if !files.contains(&file) => files.push(file),
                Source::Inline(map) => inline.push(serde_json::json!({ HOOKS: map })),
                Source::File(_) | Source::Outside(_) => {}
            }
        }
        files
            .iter()
            .map(|file| read_json(file))
            .chain(inline)
            .collect()
    }

    pub(super) fn skipped_mods(&self) -> Vec<String> {
        let docs = self.hook_docs();
        docs.iter()
            .flat_map(hooks::handlers)
            .filter(|(_, _, handler)| handler["type"] != "command")
            .map(|(event, _, handler)| {
                let kind = handler["type"].as_str().unwrap_or("hook");
                format!(
                    "skipped\t{} {kind} (summonr runs command hooks only)",
                    event.claude()
                )
            })
            .collect()
    }

    pub(crate) fn skill_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = vec![self.dir.join("skills")];
        dirs.extend(self.files("skills"));
        let mut seen = BTreeSet::new();
        dirs.retain(|dir| dir.is_dir() && seen.insert(dir.clone()));
        dirs
    }

    pub(crate) fn bin(&self) -> Option<PathBuf> {
        Some(self.dir.join("bin")).filter(|dir| dir.is_dir())
    }

    pub(crate) fn vars<'a>(&'a self, workspace: &'a Path) -> Vars<'a> {
        Vars {
            plugin: Some(self),
            workspace: Some(workspace),
            user_config: None,
        }
    }

    /// The `plugin:name` identifier Claude Code gives a component of this plugin.
    pub(crate) fn scoped(&self, name: &str) -> String {
        format!("{}:{name}", self.name)
    }

    /// The MCP server name Claude Code gives `server`, which also prefixes its tools.
    pub(crate) fn mcp_name(&self, server: &str) -> String {
        summonr_core::text::clean_name(
            &format!("plugin_{}_{server}", self.name),
            "_-",
            '_',
            usize::MAX,
        )
    }

    /// The `<plugin>-<server>` name earlier releases used, still read from config keys.
    pub(crate) fn legacy_mcp_name(&self, server: &str) -> String {
        format!("{}-{server}", self.name)
    }

    /// Server configs keyed by name: the default file, then each manifest declaration in order.
    pub(crate) fn merged(&self, merge: Merge) -> BTreeMap<String, Value> {
        let default = read_json(&self.dir.join(merge.file));
        let mut maps = vec![if merge.wrapped {
            default[merge.key].clone()
        } else {
            default
        }];
        for source in self.declared(merge.key).into_iter().flatten() {
            match source {
                Source::File(path) if is_json(&path) => {
                    let file = read_json(&path);
                    maps.push(file.get(merge.key).cloned().unwrap_or(file));
                }
                Source::Inline(map) => maps.push(map),
                Source::File(_) | Source::Outside(_) => {}
            }
        }
        maps.iter()
            .filter_map(Value::as_object)
            .flatten()
            .map(|(name, entry)| (name.clone(), entry.clone()))
            .collect()
    }

    /// Every MCP server config this plugin declares, `mcp.toml` last.
    pub(crate) fn mcp(&self) -> BTreeMap<String, Value> {
        let mut servers = self.merged(MCP);
        let native = std::fs::read_to_string(self.dir.join("mcp.toml")).ok();
        let native = native
            .and_then(|text| toml::from_str::<toml::Value>(&text).ok())
            .and_then(|file| serde_json::to_value(file.get("mcp")?).ok());
        if let Some(Value::Object(table)) = native {
            servers.extend(table);
        }
        servers
    }

    pub(crate) fn servers<T: DeserializeOwned>(
        &self,
        map: BTreeMap<String, Value>,
        name: impl Fn(&str) -> String,
        workspace: &Path,
        fields: impl Fn(&mut T) -> Vec<&mut String>,
    ) -> Vec<(String, T)> {
        map.into_iter()
            .filter_map(|(server, entry)| {
                let mut entry = T::deserialize(entry).ok()?;
                for text in fields(&mut entry) {
                    *text = expand(text, &self.vars(workspace));
                }
                Some((name(&server), entry))
            })
            .collect()
    }

    pub(crate) fn hooks(&self) -> Hooks {
        let native = std::fs::read_to_string(self.dir.join("hooks.toml")).ok();
        let native = native.and_then(|text| toml::from_str::<Hooks>(&text).ok());
        let mut hooks = Hooks::default();
        for source in self.hook_docs().iter().map(hooks::claude).chain(native) {
            for event in HookEvent::ALL {
                for hook in source.of(event) {
                    hooks.push(
                        event,
                        Hook {
                            root: Some(self.dir.clone()),
                            ..hook.clone()
                        },
                    );
                }
            }
        }
        hooks
    }
}

pub(super) fn is_json(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "json")
}

pub(super) fn read_json(path: &Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null)
}

/// Parses `path` when it holds text; a missing or blank file is `None`.
pub(crate) fn read_parsed<T, E: std::fmt::Display>(
    path: &Path,
    parse: impl FnOnce(&str) -> std::result::Result<T, E>,
) -> Result<Option<T>> {
    let failed =
        |error: &dyn std::fmt::Display| Error::Config(format!("{}: {error}", path.display()));
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(failed(&error)),
    };
    if text.trim().is_empty() {
        return Ok(None);
    }
    parse(&text).map(Some).map_err(|error| failed(&error))
}

pub(super) fn json_file(path: &Path) -> Result<Value> {
    Ok(read_parsed(path, |text: &str| serde_json::from_str::<Value>(text))?.unwrap_or(Value::Null))
}

pub(super) fn root() -> Result<PathBuf> {
    Ok(summonr_home()?.join("plugins"))
}

pub(crate) fn data_dir(name: &str) -> Result<PathBuf> {
    Ok(summonr_home()?.join("plugin-data").join(name))
}

pub fn installed() -> Vec<Plugin> {
    let Ok(root) = root() else {
        return Vec::new();
    };
    let mut plugins: Vec<Plugin> = summonr_core::walk::children(&root)
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

pub fn enabled() -> Vec<Plugin> {
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
        .into_iter()
        .flat_map(|home| summonr_core::walk::children(&home.join("bin")))
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
