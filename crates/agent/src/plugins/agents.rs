use std::path::Path;

use super::{Plugin, enabled};
use crate::skills;
use summonr_core::text::Frontmatter;

const DIR: &str = "agents";
const CLAUDE_DIR: &str = ".claude/agents";
const HEADER: &str =
    "Agents (pass one as `agent` to the task tool; the child follows its instructions):";

/// Where an agent file came from, highest priority first.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Origin {
    Project,
    User,
    Plugin(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Agent {
    pub(crate) origin: Origin,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) tools: Option<Vec<String>>,
    pub(crate) disallowed: Option<Vec<String>>,
    pub(crate) model: Option<String>,
    pub(crate) color: Option<String>,
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

    /// The frontmatter fields this agent sets that a summonr child does not apply.
    pub(crate) fn unapplied(&self) -> Vec<&'static str> {
        [
            ("model", self.model.is_some()),
            ("color", self.color.is_some()),
            (
                "disallowedTools",
                self.disallowed.is_some() && self.tools.is_none(),
            ),
        ]
        .into_iter()
        .filter_map(|(field, set)| set.then_some(field))
        .collect()
    }
}

#[must_use]
pub(crate) fn agents(workspace: &Path) -> Vec<Agent> {
    let mut found = of_dir(&workspace.join(CLAUDE_DIR), &Origin::Project);
    if let Some(home) = summonr_core::dirs::user_home() {
        found.extend(of_dir(&home.join(CLAUDE_DIR), &Origin::User));
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
    all.iter()
        .map(|agent| {
            let shared = all.iter().filter(|other| other.name == agent.name).count() > 1;
            if shared && agent.is_plugin() {
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

pub(super) fn of_plugin(plugin: &Plugin, workspace: &Path) -> Vec<Agent> {
    let build = |path: String, front: Frontmatter<'_>| {
        let (folders, stem) = split(&path);
        let scope = if folders.is_empty() {
            plugin.name.clone()
        } else {
            plugin.scoped(folders)
        };
        let body = super::expand(front.body.trim(), &plugin.vars(workspace));
        agent(Origin::Plugin(scope), stem, &front, body)
    };
    if plugin.declared(DIR).is_none() {
        return skills::markdown_tree(&plugin.dir.join(DIR), build);
    }
    plugin
        .files(DIR)
        .iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .filter_map(|path| {
            let stem = path.file_stem()?.to_str()?.to_string();
            skills::markdown_file(path, stem, build)
        })
        .collect()
}

fn of_dir(dir: &Path, origin: &Origin) -> Vec<Agent> {
    skills::markdown_tree(dir, |path, front| {
        let (_, stem) = split(&path);
        agent(origin.clone(), stem, &front, front.body.trim().to_string())
    })
}

fn split(path: &str) -> (&str, &str) {
    path.rsplit_once(':').unwrap_or(("", path))
}

fn agent(
    origin: Origin,
    stem: &str,
    front: &Frontmatter<'_>,
    instructions: String,
) -> Option<Agent> {
    let name = front.get("name").unwrap_or(stem).to_string();
    let denied = front.list("disallowedTools");
    skills::valid(&name).then(|| Agent {
        origin,
        name,
        description: front.get("description").unwrap_or_default().to_string(),
        tools: front.list("tools").map(|tools| {
            tools
                .into_iter()
                .filter(|tool| !denied.iter().flatten().any(|deny| same_tool(tool, deny)))
                .collect()
        }),
        disallowed: denied,
        model: front.get("model").map(str::to_string),
        color: front.get("color").map(str::to_string),
        instructions,
    })
}

fn same_tool(tool: &str, deny: &str) -> bool {
    let deny = deny.split_once('(').map_or(deny, |(head, _)| head).trim();
    tool == deny
        || crate::hooks::builtin(tool).is_some_and(|kind| crate::hooks::builtin(deny) == Some(kind))
}

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
