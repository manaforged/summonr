use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};

use summonr_core::dirs::summonr_home;
use summonr_core::error::{Error, Result};

const SKILL: &str = "SKILL.md";
use summonr_core::text::{FENCE, Frontmatter};

/// How many levels deep discovery looks under Summonr's own skill roots.
const DEPTH: usize = 4;
/// The one level other tools' skill roots hold, read as those tools read them.
const FLAT: usize = 1;

/// Sets every config has, each with no skills: `none` turns skills off for a session.
const BUILT_IN: [&str; 1] = ["none"];

/// A named choice of skills: folder paths to include, then paths to leave out.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize)]
pub struct Set {
    #[serde(skip)]
    name: String,
    #[serde(default)]
    include: Vec<String>,
    #[serde(default)]
    exclude: Vec<String>,
}

impl Set {
    /// The set `name` from `[skills.sets.NAME]` in config.toml, or a built-in set.
    pub fn named(name: &str) -> Result<Self> {
        Self::from_sets(&crate::config::file()?.skills.sets, name)
    }

    pub(crate) fn from_sets(sets: &BTreeMap<String, Self>, name: &str) -> Result<Self> {
        let set = match sets.get(name) {
            Some(set) => set.clone(),
            None if BUILT_IN.contains(&name) => Self::default(),
            None => {
                return Err(Error::Config(format!(
                    "no skill set `{name}`; add [skills.sets.{name}] to config.toml"
                )));
            }
        };
        Ok(Self {
            name: name.to_owned(),
            ..set
        })
    }

    /// The name the set was chosen by.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    fn admits(&self, skill: &Found) -> bool {
        let path = skill.path();
        let matches = |globs: &[String]| {
            globs
                .iter()
                .any(|glob| crate::policy::path_matches(&path, glob))
        };
        matches(&self.include) && !matches(&self.exclude)
    }
}

/// One skill found on disk: its name as callers use it, its folder path, and its file.
struct Found {
    name: String,
    folder: String,
    file: PathBuf,
}

impl Found {
    /// The skill's folder path and directory name, such as `review/rust`.
    fn path(&self) -> String {
        let leaf = self.name.rsplit(':').next().unwrap_or(&self.name);
        if self.folder.is_empty() {
            leaf.to_owned()
        } else {
            format!("{}/{leaf}", self.folder)
        }
    }
}

/// Every set a session can choose: the built-in sets, then the `[skills.sets]` names.
pub fn set_names() -> Result<Vec<String>> {
    let sets = &crate::config::file()?.skills.sets;
    let mut names: Vec<String> = BUILT_IN
        .iter()
        .map(|name| (*name).to_owned())
        .filter(|name| !sets.contains_key(name))
        .collect();
    names.extend(sets.keys().cloned());
    Ok(names)
}

/// The set a new session starts with when nothing else names one, from `[skills] default`.
pub fn default_set() -> Result<Option<String>> {
    Ok(crate::config::file()?.skills.default.clone())
}

/// Makes `name` the set new sessions start with; `None` gives them every skill.
pub fn choose_default(name: Option<&str>) -> Result<()> {
    if let Some(name) = name {
        Set::named(name)?;
    }
    crate::config::save_default_skills(&crate::config::path()?, name)
}

/// Every skill's name and summary, grouped by its folder path; `""` holds skills at a root.
#[must_use]
pub fn by_folder(workspace: &Path) -> BTreeMap<String, Vec<(String, String)>> {
    let mut folders: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for skill in found(workspace) {
        let about = about(&skill.file);
        folders
            .entry(skill.folder)
            .or_default()
            .push((skill.name, about));
    }
    folders
}

#[must_use]
pub fn list(workspace: &Path) -> Vec<(String, String)> {
    found(workspace)
        .into_iter()
        .map(|skill| {
            let about = about(&skill.file);
            (skill.name, about)
        })
        .collect()
}

/// The file that defines skill `name`, by the same search order as `list`.
#[must_use]
pub fn file(workspace: &Path, name: &str) -> Option<PathBuf> {
    found(workspace)
        .into_iter()
        .find(|skill| skill.name == name)
        .map(|skill| skill.file)
}

#[must_use]
pub(crate) fn catalog(workspace: &Path, set: Option<&Set>) -> Option<String> {
    let skills: Vec<Found> = found(workspace)
        .into_iter()
        .filter(|skill| set.is_none_or(|set| set.admits(skill)))
        .collect();
    if skills.is_empty() {
        return None;
    }
    let mut out = String::from(
        "Skills (load with the skill tool, then follow the file). Catalog only; bodies stay on disk:",
    );
    for skill in skills {
        out.push_str("\n- ");
        out.push_str(&skill.name);
        if !skill.folder.is_empty() && !skill.name.contains(':') {
            out.push_str(&format!(" [{}]", skill.folder));
        }
        let about = about(&skill.file);
        if !about.is_empty() {
            out.push_str(": ");
            out.push_str(&about);
        }
    }
    Some(out)
}

pub(crate) fn load(workspace: &Path, set: Option<&Set>, name: &str) -> Result<String> {
    let name = name.trim();
    if !reference(name) {
        return Err(Error::Tool(
            "skill names are letters, digits, `-`, and `_` only, after an optional `folder:`"
                .to_string(),
        ));
    }
    let named: Vec<Found> = found(workspace)
        .into_iter()
        .filter(|skill| skill.name == name)
        .collect();
    if let Some(set) =
        set.filter(|set| !named.is_empty() && !named.iter().any(|skill| set.admits(skill)))
    {
        return Err(Error::Tool(format!(
            "skill `{name}` is not in skill set `{}`",
            set.name()
        )));
    }
    named
        .into_iter()
        .find_map(|skill| {
            std::fs::read_to_string(&skill.file)
                .ok()
                .filter(|text| !text.trim().is_empty())
        })
        .ok_or_else(|| Error::Tool(format!("skill `{name}` is not installed")))
}

/// Every skill under the roots, first root first, named by its directory or `folder:name` on a clash.
fn found(workspace: &Path) -> Vec<Found> {
    let mut all = Vec::new();
    for (root, depth) in roots(workspace) {
        walk(&root, "", depth, &mut all);
    }
    let mut kept: Vec<Found> = Vec::new();
    for skill in all {
        if valid(&skill.name)
            && !kept
                .iter()
                .any(|seen| seen.name == skill.name && seen.folder == skill.folder)
        {
            kept.push(skill);
        }
    }
    let names: Vec<String> = kept.iter().map(|skill| skill.name.clone()).collect();
    for skill in &mut kept {
        let shared = names.iter().filter(|name| **name == skill.name).count() > 1;
        if shared && !skill.folder.is_empty() {
            skill.name = format!("{}:{}", skill.folder, skill.name);
        }
    }
    kept.sort_by(|left, right| left.name.cmp(&right.name));
    kept
}

fn walk(dir: &Path, folder: &str, depth: usize, out: &mut Vec<Found>) {
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
                out.push(Found {
                    name,
                    folder: folder.to_owned(),
                    file,
                });
            } else if depth > 1 && valid(&name) {
                let inner = if folder.is_empty() {
                    name
                } else {
                    format!("{folder}/{name}")
                };
                walk(&path, &inner, depth - 1, out);
            }
        } else if folder.is_empty() && path.extension().is_some_and(|ext| ext == "md") {
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default();
            out.push(Found {
                name: stem.to_owned(),
                folder: String::new(),
                file: path,
            });
        }
    }
}

fn about(file: &Path) -> String {
    std::fs::File::open(file)
        .ok()
        .and_then(|handle| head(std::io::BufReader::new(handle)))
        .map(|text| summary(&text))
        .unwrap_or_default()
}

/// Whether `name` is a skill name, optionally after a `folder/path:` prefix.
fn reference(name: &str) -> bool {
    match name.rsplit_once(':') {
        Some((folder, leaf)) => valid(leaf) && folder.split('/').all(valid),
        None => valid(name),
    }
}

pub(crate) fn valid(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

pub(crate) fn markdown_entries<T>(
    dir: &Path,
    build: impl Fn(String, Frontmatter<'_>) -> Option<T>,
) -> Vec<T> {
    summonr_core::walk::children(dir)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "md"))
        .filter_map(|path| {
            let stem = path.file_stem()?.to_str()?.to_string();
            let text = std::fs::read_to_string(&path).ok()?;
            build(stem, Frontmatter::parse(&text))
        })
        .collect()
}

fn head(mut reader: impl BufRead) -> Option<String> {
    let mut text = String::new();
    let mut line = Vec::new();
    let mut fenced = false;
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line).ok()? == 0 {
            return Some(text);
        }
        let piece = std::str::from_utf8(&line).ok()?;
        text.push_str(piece);
        let trimmed = piece.trim();
        if trimmed.is_empty() {
            continue;
        }
        if fenced || trimmed != FENCE {
            if !fenced || piece.trim_end() == FENCE {
                return Some(text);
            }
            continue;
        }
        fenced = true;
    }
}

fn summary(text: &str) -> String {
    let first = text.lines().map(str::trim).find(|line| !line.is_empty());
    match first {
        None => String::new(),
        Some(line) if line != FENCE => heading(line),
        Some(_) => Frontmatter::parse(text)
            .get("description")
            .map_or_else(|| heading(FENCE), summonr_core::text::squash),
    }
}

fn heading(line: &str) -> String {
    line.trim_start_matches('#')
        .trim()
        .chars()
        .take(80)
        .collect()
}

/// Each skill root in search order, with how many levels deep it holds skills.
fn roots(workspace: &Path) -> Vec<(PathBuf, usize)> {
    let mut roots: Vec<(PathBuf, usize)> = summonr_home()
        .ok()
        .map(|home| (home.join("skills"), DEPTH))
        .into_iter()
        .collect();
    if let Some(home) = summonr_core::dirs::user_home() {
        roots.push((home.join(".claude/skills"), FLAT));
        roots.push((home.join(".agents/skills"), FLAT));
    }
    roots.push((workspace.join(".summonr").join("skills"), DEPTH));
    roots.push((workspace.join(".claude/skills"), FLAT));
    roots.extend(plugin_dirs().into_iter().map(|dir| (dir, FLAT)));
    roots
}

fn plugin_dirs() -> Vec<PathBuf> {
    crate::plugins::enabled()
        .iter()
        .flat_map(crate::plugins::Plugin::skill_dirs)
        .collect()
}

#[cfg(test)]
#[path = "skills_tests.rs"]
mod tests;
