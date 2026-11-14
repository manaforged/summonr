use std::path::{Path, PathBuf};

use summonr_core::dirs::summonr_home;

use super::meta::Meta;
use super::{DEPTH, FLAT, SEPARATOR, SKILL, valid};
use crate::plugins::Plugin;

const SKILLS_KEY: &str = "skills";

/// One skill found on disk: the name a caller types, its directory name, and where it came from.
pub(super) struct Found {
    pub(super) name: String,
    pub(super) alias: Option<String>,
    pub(super) leaf: String,
    pub(super) folder: String,
    pub(super) file: PathBuf,
    pub(super) plugin: Option<Plugin>,
    pub(super) meta: Meta,
}

impl Found {
    fn new(leaf: String, folder: &str, file: PathBuf, plugin: Option<&Plugin>) -> Self {
        Self {
            name: leaf.clone(),
            alias: None,
            meta: Meta::read(&file),
            leaf,
            folder: folder.to_owned(),
            file,
            plugin: plugin.cloned(),
        }
    }

    /// The skill's folder path and directory name, such as `review/rust`.
    pub(super) fn path(&self) -> String {
        if self.folder.is_empty() {
            self.leaf.clone()
        } else {
            format!("{}/{}", self.folder, self.leaf)
        }
    }

    /// Whether typing `name` reaches this skill.
    pub(super) fn answers(&self, name: &str) -> bool {
        self.name == name || self.alias.as_deref() == Some(name)
    }

    /// The file's text with the plugin, project, skill folder, and session placeholders filled.
    pub(super) fn content(&self, text: &str, workspace: &Path, session: Option<&str>) -> String {
        crate::plugins::commands::content(
            text,
            self.plugin.as_ref(),
            self.file.parent(),
            workspace,
            session,
        )
    }
}

struct Root {
    dir: PathBuf,
    depth: usize,
    plugin: Option<Plugin>,
}

/// Every skill under the roots, first root first; plugin skills are `plugin:name` and keep a bare alias when it is free.
pub(super) fn found(workspace: &Path) -> Vec<Found> {
    let mut all = Vec::new();
    for root in roots(workspace) {
        gather(&root, &mut all);
    }
    let (plugged, local): (Vec<Found>, Vec<Found>) =
        all.into_iter().partition(|skill| skill.plugin.is_some());
    let mut kept = local_names(local);
    kept.extend(plugin_names(plugged));
    free_aliases(&mut kept);
    kept.sort_by(|left, right| left.name.cmp(&right.name));
    kept
}

fn local_names(all: Vec<Found>) -> Vec<Found> {
    let mut kept: Vec<Found> = Vec::new();
    for mut skill in all {
        let chosen = skill
            .meta
            .name
            .clone()
            .filter(|name| valid(name))
            .unwrap_or_else(|| skill.leaf.clone());
        if !valid(&chosen)
            || kept
                .iter()
                .any(|seen| seen.name == chosen && seen.folder == skill.folder)
        {
            continue;
        }
        skill.alias = (chosen != skill.leaf && valid(&skill.leaf)).then(|| skill.leaf.clone());
        skill.name = chosen;
        kept.push(skill);
    }
    let names: Vec<String> = kept.iter().map(|skill| skill.name.clone()).collect();
    for skill in &mut kept {
        let shared = names.iter().filter(|name| **name == skill.name).count() > 1;
        if shared && !skill.folder.is_empty() {
            skill.name = format!("{}{SEPARATOR}{}", skill.folder, skill.name);
        }
    }
    kept
}

fn plugin_names(all: Vec<Found>) -> Vec<Found> {
    let mut kept: Vec<Found> = Vec::new();
    for mut skill in all {
        let Some(plugin) = skill.plugin.as_ref() else {
            continue;
        };
        let prefix = plugin.scoped("");
        let leaf = skill
            .meta
            .name
            .as_deref()
            .map(|name| name.strip_prefix(&prefix).unwrap_or(name))
            .filter(|name| valid(name))
            .map_or_else(|| skill.leaf.clone(), str::to_owned);
        let name = plugin.scoped(&leaf);
        if !valid(&leaf) || kept.iter().any(|seen| seen.name == name) {
            continue;
        }
        skill.name = name;
        skill.alias = Some(leaf);
        kept.push(skill);
    }
    kept
}

fn free_aliases(all: &mut [Found]) {
    let taken: Vec<String> = all
        .iter()
        .flat_map(|skill| std::iter::once(skill.name.clone()).chain(skill.alias.clone()))
        .collect();
    for skill in all {
        skill.alias = skill.alias.take().filter(|alias| {
            taken.iter().filter(|name| *name == alias).count() == 1
                && crate::parse_command(&format!("/{alias}")).is_none()
        });
    }
}

fn gather(root: &Root, out: &mut Vec<Found>) {
    if let Some(plugin) = &root.plugin
        && root.dir.join(SKILL).is_file()
    {
        let leaf = if root.dir == plugin.dir {
            Some(plugin.name.clone())
        } else {
            root.dir
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        };
        out.extend(leaf.map(|leaf| Found::new(leaf, "", root.dir.join(SKILL), Some(plugin))));
        return;
    }
    walk(&root.dir, "", root.depth, root.plugin.as_ref(), out);
}

fn walk(dir: &Path, folder: &str, depth: usize, plugin: Option<&Plugin>, out: &mut Vec<Found>) {
    for entry in summonr_core::walk::children(dir) {
        let path = entry.path();
        let Some(name) = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
        else {
            continue;
        };
        if std::fs::metadata(&path).is_ok_and(|meta| meta.is_dir()) {
            let file = path.join(SKILL);
            if file.is_file() {
                out.push(Found::new(name, folder, file, plugin));
            } else if depth > 1 && valid(&name) {
                let inner = if folder.is_empty() {
                    name
                } else {
                    format!("{folder}/{name}")
                };
                walk(&path, &inner, depth - 1, plugin, out);
            }
        } else if plugin.is_none()
            && folder.is_empty()
            && path.extension().is_some_and(|ext| ext == "md")
        {
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default()
                .to_owned();
            out.push(Found::new(stem, "", path, plugin));
        }
    }
}

fn roots(workspace: &Path) -> Vec<Root> {
    let local = |dir: PathBuf, depth: usize| Root {
        dir,
        depth,
        plugin: None,
    };
    let mut roots: Vec<Root> = summonr_home()
        .ok()
        .map(|home| local(home.join("skills"), DEPTH))
        .into_iter()
        .collect();
    if let Some(home) = summonr_core::dirs::user_home() {
        roots.push(local(
            summonr_core::dirs::claude(&home).join("skills"),
            FLAT,
        ));
        roots.push(local(home.join(".agents/skills"), FLAT));
    }
    roots.push(local(
        summonr_core::dirs::workspace(workspace).join("skills"),
        DEPTH,
    ));
    roots.push(local(
        summonr_core::dirs::claude(workspace).join("skills"),
        FLAT,
    ));
    for plugin in crate::plugins::enabled() {
        roots.extend(plugin_roots(&plugin));
    }
    roots
}

fn plugin_roots(plugin: &Plugin) -> Vec<Root> {
    let mut dirs = plugin.skill_dirs();
    if dirs.is_empty()
        && plugin.manifest.get(SKILLS_KEY).is_none()
        && plugin.dir.join(SKILL).is_file()
    {
        dirs.push(plugin.dir.clone());
    }
    dirs.into_iter()
        .map(|dir| Root {
            dir,
            depth: FLAT,
            plugin: Some(plugin.clone()),
        })
        .collect()
}

/// Each `SKILL.md` file `plugin` adds, by the rules discovery uses.
pub(crate) fn plugin_files(plugin: &Plugin) -> Vec<PathBuf> {
    let mut all = Vec::new();
    for root in plugin_roots(plugin) {
        gather(&root, &mut all);
    }
    let mut files: Vec<PathBuf> = all.into_iter().map(|skill| skill.file).collect();
    files.sort();
    files.dedup();
    files
}
