use std::path::Path;

use super::{Plugin, enabled};
use crate::skills;

const DIR: &str = "agents";
const HEADER: &str =
    "Agents (pass one as `agent` to the task tool; the child follows its instructions):";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Agent {
    pub(crate) plugin: String,
    pub(crate) name: String,
    pub(crate) description: String,
    pub(crate) tools: Option<Vec<String>>,
    pub(crate) instructions: String,
}

impl Agent {
    fn qualified(&self) -> String {
        format!("{}:{}", self.plugin, self.name)
    }
}

#[must_use]
pub(crate) fn agents(workspace: &Path) -> Vec<Agent> {
    let mut found: Vec<Agent> = enabled()
        .iter()
        .flat_map(|plugin| of_plugin(plugin, workspace))
        .collect();
    found.sort_by(|left, right| (&left.name, &left.plugin).cmp(&(&right.name, &right.plugin)));
    found.dedup_by(|left, right| left.name == right.name && left.plugin == right.plugin);
    found
}

pub(crate) fn find(name: &str, workspace: &Path) -> Result<Agent, String> {
    let name = name.trim();
    let all = agents(workspace);
    let mut matched: Vec<&Agent> = all
        .iter()
        .filter(|agent| agent.name == name || agent.qualified() == name)
        .collect();
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
            if all.iter().filter(|other| other.name == agent.name).count() > 1 {
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
    skills::markdown_entries(&plugin.dir.join(DIR), |stem, front| {
        let name = front.get("name").map_or(stem, str::to_string);
        skills::valid(&name).then(|| Agent {
            plugin: plugin.name.clone(),
            name,
            description: front.get("description").unwrap_or_default().to_string(),
            tools: front.list("tools"),
            instructions: super::expand(front.body.trim(), &plugin.vars(workspace)),
        })
    })
}

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
