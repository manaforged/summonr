use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use summonr_core::error::{Error, Result};
use summonr_core::text::Frontmatter;

mod fill;
mod found;
mod inject;
mod meta;

pub(crate) use found::plugin_files;
use found::{Found, found};
pub(crate) use inject::{Body, Piece, Scope};
pub(crate) use meta::{Meta, flag, unapplied};

const SKILL: &str = "SKILL.md";
/// The mark between a namespace and a name, as in `plugin:skill` or `frontend:component`.
pub(crate) const SEPARATOR: char = ':';

/// How many levels deep discovery looks under Summonr's own skill and command roots.
const DEPTH: usize = 4;
/// The one level other tools' skill roots hold, read as those tools read them.
const FLAT: usize = 1;

/// Sets every config has, each with no skills: `none` turns skills off for a session.
const BUILT_IN: [&str; 1] = ["none"];

static NOTED: AtomicBool = AtomicBool::new(false);

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
    /// The set `name` from `[skills.sets.NAME]` in config.toml, a built-in set, or skill folder paths such as `acme,personal/review`.
    pub fn named(name: &str, workspace: &Path) -> Result<Self> {
        Self::resolve(&crate::config::file()?.skills.sets, name, workspace)
    }

    pub(crate) fn resolve(
        sets: &BTreeMap<String, Self>,
        name: &str,
        workspace: &Path,
    ) -> Result<Self> {
        if sets.contains_key(name) || BUILT_IN.contains(&name) {
            return Self::from_sets(sets, name);
        }
        let folders = Self {
            name: name.to_owned(),
            include: name
                .split(',')
                .map(str::trim)
                .filter(|path| !path.is_empty())
                .map(str::to_owned)
                .collect(),
            exclude: Vec::new(),
        };
        if found(workspace).iter().any(|skill| folders.admits(skill)) {
            return Ok(folders);
        }
        Self::from_sets(sets, name)
    }

    pub(crate) fn from_sets(sets: &BTreeMap<String, Self>, name: &str) -> Result<Self> {
        let set = match sets.get(name) {
            Some(set) => set.clone(),
            None if BUILT_IN.contains(&name) => Self::default(),
            None => {
                return Err(Error::Config(format!(
                    "no skill set or skill folder `{name}`; add [skills.sets.{name}] to config.toml"
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
        Set::from_sets(&crate::config::file()?.skills.sets, name)?;
    }
    crate::config::save_default_skills(&crate::config::path()?, name)
}

/// Every skill's name and summary, grouped by its folder path; `""` holds skills at a root.
#[must_use]
pub fn by_folder(workspace: &Path) -> BTreeMap<String, Vec<(String, String)>> {
    let mut folders: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for skill in found(workspace) {
        folders
            .entry(skill.folder)
            .or_default()
            .push((skill.name, skill.meta.about));
    }
    folders
}

/// Every name a person can type as `/name` to run a skill, with its summary.
#[must_use]
pub fn list(workspace: &Path) -> Vec<(String, String)> {
    entries(workspace)
        .into_iter()
        .map(|entry| (entry.name, entry.about))
        .collect()
}

/// One name in the `/` menu: what to type, its summary and argument hint, and whether it is a bare alias.
pub(crate) struct Entry {
    pub(crate) name: String,
    pub(crate) about: String,
    pub(crate) offer: String,
    pub(crate) alias: bool,
}

/// Every `/` menu name for a skill: each canonical name, and each bare alias that is free.
pub(crate) fn entries(workspace: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    for skill in found(workspace)
        .into_iter()
        .filter(|skill| skill.meta.user_invocable)
    {
        let offer = skill.meta.offer();
        out.extend(skill.alias.iter().map(|alias| Entry {
            name: alias.clone(),
            about: skill.meta.about.clone(),
            offer: offer.clone(),
            alias: true,
        }));
        out.push(Entry {
            name: skill.name,
            about: skill.meta.about,
            offer,
            alias: false,
        });
    }
    out
}

/// The file that defines skill `name`, by the same search order as `list`.
#[must_use]
pub fn file(workspace: &Path, name: &str) -> Option<PathBuf> {
    found(workspace)
        .into_iter()
        .find(|skill| skill.answers(name))
        .map(|skill| skill.file)
}

#[must_use]
pub(crate) fn catalog(workspace: &Path, set: Option<&Set>) -> Option<String> {
    let skills: Vec<Found> = found(workspace)
        .into_iter()
        .filter(|skill| skill.meta.model_invocable && set.is_none_or(|set| set.admits(skill)))
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
        if !skill.folder.is_empty() && !skill.name.contains(SEPARATOR) {
            out.push_str(&format!(" [{}]", skill.folder));
        }
        if !skill.meta.about.is_empty() {
            out.push_str(": ");
            out.push_str(&skill.meta.about);
        }
    }
    Some(out)
}

pub(crate) fn load(
    workspace: &Path,
    set: Option<&Set>,
    name: &str,
    session: Option<&str>,
) -> Result<Body> {
    let name = name.trim();
    if !reference(name) {
        return Err(Error::Tool(
            "skill names are letters, digits, `-`, and `_` only, after an optional `folder:`"
                .to_string(),
        ));
    }
    let named: Vec<Found> = found(workspace)
        .into_iter()
        .filter(|skill| skill.answers(name))
        .collect();
    if let Some(set) =
        set.filter(|set| !named.is_empty() && !named.iter().any(|skill| set.admits(skill)))
    {
        return Err(Error::Tool(format!(
            "skill `{name}` is not in skill set `{}`",
            set.name()
        )));
    }
    if !named.is_empty() && named.iter().all(|skill| !skill.meta.model_invocable) {
        return Err(Error::Tool(format!(
            "skill `{name}` runs only when the user types /{name}"
        )));
    }
    named
        .into_iter()
        .filter(|skill| skill.meta.model_invocable)
        .find_map(|skill| {
            written(&skill.file)
                .ok()
                .filter(|body| !body.is_empty())
                .map(|body| skill.body(&body, None, workspace, session))
        })
        .ok_or_else(|| Error::Tool(format!("skill `{name}` is not installed")))
}

/// Prints once per process each frontmatter key that the user's and the workspace's own skills and commands set and summonr does not apply.
pub(crate) fn report(workspace: &Path) {
    if NOTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let skills = found(workspace)
        .into_iter()
        .filter(|skill| skill.plugin.is_none())
        .flat_map(|skill| skill.meta.notes("skill", &skill.name));
    for note in skills.chain(crate::plugins::commands::notes(workspace)) {
        eprintln!("summonr: {note}");
    }
}

/// A skill a person ran by typing its name.
pub(crate) struct Typed {
    skill: Found,
    pub(crate) alias: bool,
}

impl Typed {
    /// The skill body with `arguments`, the path placeholders, and the session id filled.
    pub(crate) fn render(
        &self,
        arguments: &str,
        workspace: &Path,
        session: Option<&str>,
    ) -> Result<Body> {
        let name = &self.skill.name;
        let body = written(&self.skill.file)
            .map_err(|error| Error::Tool(format!("skill `{name}`: {error}")))?;
        if body.is_empty() {
            return Err(Error::Tool(format!("skill `{name}` is not installed")));
        }
        let arguments = (arguments, self.skill.meta.arguments.as_slice());
        Ok(self.skill.body(&body, Some(arguments), workspace, session))
    }
}

fn written(file: &Path) -> std::io::Result<String> {
    let text = std::fs::read_to_string(file)?;
    Ok(Frontmatter::parse(&text).body.trim().to_owned())
}

/// The skill a person runs by typing `/name`, unless it sets `user-invocable: false`.
pub(crate) fn typed(workspace: &Path, name: &str) -> Option<Typed> {
    found(workspace)
        .into_iter()
        .find(|skill| skill.meta.user_invocable && skill.answers(name))
        .map(|skill| Typed {
            alias: skill.name != name,
            skill,
        })
}

/// Whether `name` is a skill name, optionally after a `folder/path:` prefix.
fn reference(name: &str) -> bool {
    match name.rsplit_once(SEPARATOR) {
        Some((folder, leaf)) => valid(leaf) && folder.split('/').all(valid),
        None => valid(name),
    }
}

pub(crate) fn valid(name: &str) -> bool {
    summonr_core::text::is_name(name, summonr_core::text::Charset::Word)
}

/// Whether `path` names a `.md` file.
pub(crate) fn is_markdown(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "md")
}

/// Each Markdown file under `dir`, named by its folder path and stem joined with `:`, such as `frontend:component`.
pub(crate) fn markdown_tree<T>(
    dir: &Path,
    build: impl Fn(String, Frontmatter<'_>) -> Option<T>,
) -> Vec<T> {
    markdown(dir, "", DEPTH, &build)
}

/// The Markdown file at `path`, read as entry `name`.
pub(crate) fn markdown_file<T>(
    path: &Path,
    name: String,
    build: impl Fn(String, Frontmatter<'_>) -> Option<T>,
) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    build(name, Frontmatter::parse(&text))
}

fn markdown<T>(
    dir: &Path,
    prefix: &str,
    depth: usize,
    build: &impl Fn(String, Frontmatter<'_>) -> Option<T>,
) -> Vec<T> {
    let mut out = Vec::new();
    for path in summonr_core::walk::children(dir).map(|entry| entry.path()) {
        let folder = path.is_dir();
        let leaf = if folder {
            path.file_name()
        } else {
            path.file_stem()
        };
        let Some(leaf) = leaf.and_then(|leaf| leaf.to_str()) else {
            continue;
        };
        let name = if prefix.is_empty() {
            leaf.to_owned()
        } else {
            format!("{prefix}{SEPARATOR}{leaf}")
        };
        if folder && depth > 1 && valid(leaf) {
            out.extend(markdown(&path, &name, depth - 1, build));
        } else if !folder && path.is_file() && is_markdown(&path) {
            out.extend(markdown_file(&path, name, build));
        }
    }
    out
}

#[cfg(test)]
#[path = "skills_tests.rs"]
mod tests;
