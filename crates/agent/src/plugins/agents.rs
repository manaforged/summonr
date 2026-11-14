use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::{Plugin, Source, enabled};
use crate::skills;
use summonr_core::text::Frontmatter;

const DIR: &str = "agents";
const HEADER: &str =
    "Agents (pass one as `agent` to the task tool; the child follows its instructions):";

/// Where an agent file came from, highest priority first.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Origin {
    Project,
    User,
    Plugin(String),
}

/// A built-in model family name an agent's `model` field can give.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Alias {
    Sonnet,
    Opus,
    Haiku,
    Fable,
}

impl Alias {
    const ALL: [Self; 4] = [Self::Sonnet, Self::Opus, Self::Haiku, Self::Fable];

    /// The alias as the `model` field spells it.
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Sonnet => "sonnet",
            Self::Opus => "opus",
            Self::Haiku => "haiku",
            Self::Fable => "fable",
        }
    }
}

/// The model an agent's `model` field asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Model {
    Inherit,
    Alias(Alias),
    Id(String),
}

impl Model {
    fn parse(raw: &str) -> Self {
        let raw = raw.trim();
        if raw.eq_ignore_ascii_case("inherit") {
            return Self::Inherit;
        }
        Alias::ALL
            .into_iter()
            .find(|alias| alias.name().eq_ignore_ascii_case(raw))
            .map_or_else(|| Self::Id(raw.to_string()), Self::Alias)
    }

    /// The model name as the agent file spells it.
    pub(crate) fn text(&self) -> &str {
        match self {
            Self::Inherit => "inherit",
            Self::Alias(alias) => alias.name(),
            Self::Id(id) => id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Agent {
    pub(crate) origin: Origin,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) tools: Option<Vec<String>>,
    pub(crate) disallowed: Option<Vec<String>>,
    pub(crate) model: Option<Model>,
    pub(crate) color: Option<String>,
    pub(crate) skills: Vec<String>,
    pub(crate) instructions: String,
}

impl Agent {
    fn qualified(&self) -> String {
        match &self.origin {
            Origin::Plugin(scope) => format!("{scope}:{}", self.name),
            Origin::Project | Origin::User => self.name.clone(),
        }
    }

    fn is_plugin(&self) -> bool {
        matches!(self.origin, Origin::Plugin(_))
    }

    /// The tool scope its `tools` and `disallowedTools` fields give.
    pub(crate) fn scope(&self) -> crate::policy::Scope {
        crate::policy::Scope::new(self.tools.as_deref(), self.disallowed.as_deref())
    }

    /// The frontmatter fields this agent sets that a summonr child does not apply.
    pub(crate) fn unapplied(&self) -> Vec<&'static str> {
        [("color", self.color.is_some())]
            .into_iter()
            .filter_map(|(field, set)| set.then_some(field))
            .collect()
    }
}

#[must_use]
pub(crate) fn agents(workspace: &Path) -> Vec<Agent> {
    let mut found: Vec<Agent> = project_dirs(workspace)
        .iter()
        .flat_map(|dir| of_dir(&dir.join(DIR), &Origin::Project))
        .collect();
    if let Some(home) = summonr_core::dirs::user_home() {
        found.extend(of_dir(
            &summonr_core::dirs::claude(&home).join(DIR),
            &Origin::User,
        ));
    }
    found.extend(
        enabled()
            .iter()
            .flat_map(|plugin| of_plugin(plugin, workspace)),
    );
    found.sort_by(|left, right| {
        (&left.name, &left.origin, left.qualified()).cmp(&(
            &right.name,
            &right.origin,
            right.qualified(),
        ))
    });
    found.dedup_by(|later, kept| {
        later.qualified() == kept.qualified()
            || (later.name == kept.name && !later.is_plugin() && !kept.is_plugin())
    });
    found
}

pub(crate) fn find(name: &str, workspace: &Path) -> Result<Agent, String> {
    let name = name.trim();
    let all = agents(workspace);
    if let Some(agent) = all.iter().find(|agent| agent.qualified() == name) {
        return Ok(agent.clone());
    }
    let mut matched: Vec<&Agent> = all.iter().filter(|agent| agent.name == name).collect();
    match matched.len() {
        1 => Ok(matched.remove(0).clone()),
        0 => {
            let names = labels(&all);
            let available = if names.is_empty() {
                "none".to_string()
            } else {
                names.join(", ")
            };
            Err(format!(
                "unknown agent `{name}`; available agents: {available}"
            ))
        }
        2.. => Err(format!(
            "agent `{name}` is in more than one plugin; use one of: {}",
            matched
                .iter()
                .map(|agent| agent.qualified())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

fn labels(all: &[Agent]) -> Vec<String> {
    let names = skills::Names::count(all.iter().map(|agent| agent.name.as_str()));
    all.iter()
        .map(|agent| {
            if !names.once(&agent.name) && agent.is_plugin() {
                agent.qualified()
            } else {
                agent.name.clone()
            }
        })
        .collect()
}

#[must_use]
pub(crate) fn catalog(workspace: &Path) -> Option<String> {
    let found = agents(workspace);
    if found.is_empty() {
        return None;
    }
    let mut out = String::from(HEADER);
    for (agent, label) in found.iter().zip(labels(&found)) {
        out.push_str("\n- ");
        out.push_str(&label);
        if !agent.description.is_empty() {
            out.push_str(": ");
            out.push_str(&agent.description);
        }
    }
    Some(out)
}

/// Each `.claude` directory from `workspace` up to its repository root, closest first.
pub(crate) fn project_dirs(workspace: &Path) -> Vec<PathBuf> {
    let top = crate::diff::layout(workspace).map(|layout| layout.top);
    let start = dunce::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    let mut dirs = Vec::new();
    for dir in start.ancestors() {
        dirs.push(summonr_core::dirs::claude(dir));
        if top.as_deref().is_none_or(|top| top == dir) {
            break;
        }
    }
    dirs
}

pub(super) fn of_plugin(plugin: &Plugin, workspace: &Path) -> Vec<Agent> {
    let build = |path: String, front: Frontmatter<'_>| {
        let (folders, stem) = split(&path);
        let scope = if folders.is_empty() {
            plugin.name.clone()
        } else {
            plugin.scoped(folders)
        };
        let body = super::expand(front.body.trim(), &plugin.vars(workspace));
        let name = front.get("name").unwrap_or(stem);
        agent(Origin::Plugin(scope), name, &front, body)
    };
    plugin.components(
        DIR,
        DIR,
        |dir| skills::markdown_tree(dir, build),
        |source| match source {
            Source::File(path) if skills::is_markdown(&path) => path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .and_then(|stem| skills::markdown_file(&path, stem.to_string(), build))
                .into_iter()
                .collect(),
            Source::File(_) | Source::Inline(_) | Source::Outside(_) => Vec::new(),
        },
    )
}

fn of_dir(dir: &Path, origin: &Origin) -> Vec<Agent> {
    skills::markdown_tree(dir, |_, front| {
        front.get("description")?;
        let name = front.get("name")?;
        agent(origin.clone(), name, &front, front.body.trim().to_string())
    })
}

fn split(path: &str) -> (&str, &str) {
    path.rsplit_once(':').unwrap_or(("", path))
}

fn agent(
    origin: Origin,
    name: &str,
    front: &Frontmatter<'_>,
    instructions: String,
) -> Option<Agent> {
    let mut preload: Vec<String> = Vec::new();
    let mut seen = HashSet::new();
    for skill in front.list("skills").into_iter().flatten() {
        if preload.len() < crate::limits::limits().agent_skills_max && seen.insert(skill.clone()) {
            preload.push(skill);
        }
    }
    skills::valid(name).then(|| Agent {
        origin,
        name: name.to_string(),
        description: front.get("description").unwrap_or_default().to_string(),
        tools: front.names("tools"),
        disallowed: front.names("disallowedTools"),
        model: front.get("model").map(Model::parse),
        color: front.get("color").map(str::to_string),
        skills: preload,
        instructions,
    })
}

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
