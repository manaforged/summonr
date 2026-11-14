use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::hooks::{Hook, HookEvent, Hooks};
use crate::skills;
use summonr_core::dirs::summonr_home;
use summonr_core::error::Result;
use summonr_types::mcp::Entry;

pub(crate) mod agents;
pub(crate) mod commands;
mod expand;
mod hooks;
pub(crate) mod lsp;
pub mod manage;
pub mod market;
mod read;

pub(crate) use expand::{HOOK_VARS, USER_CONFIG, Unset, Vars, expand, hook_env, scan};
pub(crate) use hooks::{claude as claude_hooks, unsupported as unsupported_hooks};
pub(crate) use manage::off::{unknown_off, warn_unknown_off};
pub(super) use read::{is_json, json_file, read_json, read_noted, skip};
pub(crate) use read::{located, read_parsed, server_entry};

pub(super) const MANIFEST: &str = ".claude-plugin/plugin.json";
pub(super) const HOOKS: &str = "hooks";
pub(super) const HOOK_FILE: &str = "hooks/hooks.json";
pub(super) const NATIVE_HOOKS: &str = "hooks.toml";
pub(super) const NATIVE_MCP: &str = "mcp.toml";
pub(super) const UNSUPPORTED: [(&str, &str); 3] = [
    ("monitors", "monitors"),
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

/// One MCP server a plugin declares: its key, its summonr name, the file that declares it, and its config.
#[derive(Clone, Debug)]
pub(crate) struct McpServer {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) file: PathBuf,
    pub(crate) config: Value,
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

    /// The plugin installed in `dir`, named by its folder, or `None` when that name is not valid.
    pub(crate) fn open(dir: PathBuf) -> Option<Self> {
        let name = dir.file_name()?.to_str()?.to_string();
        skills::valid(&name).then(|| Self::at(name, dir))
    }

    fn inside(&self, relative: &str) -> Option<PathBuf> {
        inside(&self.dir, relative)
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

    /// Component `key`: `folder` reads the `default_dir` folder unless the manifest declares `key`, whose entries `entry` reads instead.
    pub(crate) fn components<T>(
        &self,
        key: &str,
        default_dir: &str,
        folder: impl Fn(&Path) -> Vec<T>,
        entry: impl Fn(Source) -> Vec<T>,
    ) -> Vec<T> {
        match self.declared(key) {
            None => folder(&self.dir.join(default_dir)),
            Some(sources) => sources.into_iter().flat_map(entry).collect(),
        }
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
        let default = self.dir.join(HOOK_FILE);
        let mut seen = BTreeSet::from([default.clone()]);
        let mut docs = vec![(read_json(&default), default)];
        let mut inline = Vec::new();
        for source in self.declared(HOOKS).into_iter().flatten() {
            match source {
                Source::File(file) if seen.insert(file.clone()) => {
                    docs.push((read_json(&file), file));
                }
                Source::Inline(map) => {
                    inline.push((serde_json::json!({ HOOKS: map }), self.dir.join(MANIFEST)));
                }
                Source::File(_) | Source::Outside(_) => {}
            }
        }
        docs.into_iter()
            .chain(inline)
            .filter_map(|(doc, file)| match hooks::shape(&doc) {
                Some(problem) => {
                    skip(&located(&file, problem));
                    None
                }
                None => Some(doc),
            })
            .collect()
    }

    pub(super) fn skipped_mods(&self) -> Vec<String> {
        let docs = self.hook_docs();
        docs.iter()
            .flat_map(hooks::skipped)
            .map(|(event, kind)| {
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
            ..Vars::default()
        }
    }

    /// The `plugin:name` identifier Claude Code gives a component of this plugin.
    pub(crate) fn scoped(&self, name: &str) -> String {
        format!("{}:{name}", self.name)
    }

    /// The MCP server name Claude Code gives `server`, which also prefixes its tools.
    pub(crate) fn mcp_name(&self, server: &str) -> String {
        crate::mcp::clean(&format!("plugin_{}_{server}", self.name), usize::MAX)
    }

    /// The `<plugin>-<server>` name earlier releases used, still read from config keys.
    pub(crate) fn legacy_mcp_name(&self, server: &str) -> String {
        format!("{}-{server}", self.name)
    }

    /// Each of `servers` by its legacy name, mapped to the name summonr gives it now.
    pub(crate) fn legacy_names(&self, servers: &[McpServer]) -> BTreeMap<String, String> {
        servers
            .iter()
            .map(|server| (self.legacy_mcp_name(&server.key), server.name.clone()))
            .collect()
    }

    fn maps(&self, merge: Merge) -> Vec<(PathBuf, Value)> {
        let path = self.dir.join(merge.file);
        let default = read_json(&path);
        let default = if merge.wrapped {
            default[merge.key].clone()
        } else {
            default
        };
        let mut maps = vec![(path, default)];
        for source in self.declared(merge.key).into_iter().flatten() {
            match source {
                Source::File(path) if is_json(&path) => {
                    let file = read_json(&path);
                    maps.push((path, file.get(merge.key).cloned().unwrap_or(file)));
                }
                Source::Inline(map) => maps.push((self.dir.join(MANIFEST), map)),
                Source::File(_) | Source::Outside(_) => {}
            }
        }
        maps
    }

    /// Server configs keyed by name with the file that declares each: the default file, then each manifest declaration in order.
    pub(crate) fn merged(&self, merge: Merge) -> BTreeMap<String, (PathBuf, Value)> {
        keyed(self.maps(merge))
    }

    /// Every MCP server this plugin declares, in key order, `mcp.toml` last, with the name summonr gives it.
    pub(crate) fn mcp_servers(&self) -> Vec<McpServer> {
        let mut maps = self.maps(MCP);
        let path = self.dir.join(NATIVE_MCP);
        let native = read_noted(&path, toml::from_str::<toml::Value>)
            .and_then(|file| serde_json::to_value(file.get("mcp")?).ok());
        maps.extend(native.map(|table| (path, table)));
        keyed(maps)
            .into_iter()
            .map(|(key, (file, config))| McpServer {
                name: self.mcp_name(&key),
                key,
                file,
                config,
            })
            .collect()
    }

    pub(crate) fn hooks(&self) -> Hooks {
        let native = read_noted(&self.dir.join(NATIVE_HOOKS), toml::from_str::<Hooks>);
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

/// The `[mcp.NAME]` tables of `layer` under the names `legacy` maps them to, each later table laid over an earlier one of the same name.
pub(crate) fn renamed(
    layer: BTreeMap<String, Entry>,
    legacy: &BTreeMap<String, String>,
) -> BTreeMap<String, Entry> {
    let mut out: BTreeMap<String, Entry> = BTreeMap::new();
    for (name, entry) in layer {
        let name = legacy.get(&name).cloned().unwrap_or(name);
        match out.get_mut(&name) {
            Some(base) => base.overlay(entry),
            None => {
                out.insert(name, entry);
            }
        }
    }
    out
}

fn keyed(maps: Vec<(PathBuf, Value)>) -> BTreeMap<String, (PathBuf, Value)> {
    let mut servers = BTreeMap::new();
    for (path, map) in maps {
        if let Value::Object(map) = map {
            servers.extend(
                map.into_iter()
                    .map(|(name, entry)| (name, (path.clone(), entry))),
            );
        }
    }
    servers
}

/// `relative` under `dir`, with `./` and `.` allowed, or `None` when it is absolute or has `..`.
pub(super) fn inside(dir: &Path, relative: &str) -> Option<PathBuf> {
    let relative = relative.strip_prefix("./").unwrap_or(relative);
    let relative = if relative == "." { "" } else { relative };
    let escapes =
        Path::new(relative).is_absolute() || relative.split(['/', '\\']).any(|part| part == "..");
    (!escapes).then(|| {
        if relative.is_empty() {
            dir.to_path_buf()
        } else {
            dir.join(relative)
        }
    })
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
        .filter_map(Plugin::open)
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
