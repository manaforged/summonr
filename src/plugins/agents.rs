use std::path::Path;

use super::{Plugin, enabled};
use crate::text::Frontmatter;

const DIR: &str = "agents";
const EXTENSION: &str = "md";
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

#[must_use]
pub(crate) fn agents(workspace: &Path) -> Vec<Agent> {
    let mut found: Vec<Agent> = enabled()
        .iter()
        .flat_map(|plugin| of_plugin(plugin, workspace))
        .collect();
    found.sort_by(|left, right| left.name.cmp(&right.name));
    found.dedup_by(|left, right| left.name == right.name);
    found
}

#[must_use]
pub(crate) fn find(name: &str, workspace: &Path) -> Option<Agent> {
    let name = name.trim();
    agents(workspace)
        .into_iter()
        .find(|agent| agent.name == name)
}

#[must_use]
pub(crate) fn catalog(workspace: &Path) -> Option<String> {
    let found = agents(workspace);
    if found.is_empty() {
        return None;
    }
    let mut out = String::from(HEADER);
    for agent in found {
        out.push_str("\n- ");
        out.push_str(&agent.name);
        if !agent.description.is_empty() {
            out.push_str(": ");
            out.push_str(&agent.description);
        }
    }
    Some(out)
}

fn of_plugin(plugin: &Plugin, workspace: &Path) -> Vec<Agent> {
    let Ok(entries) = std::fs::read_dir(plugin.dir.join(DIR)) else {
        return Vec::new();
    };
    entries
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == EXTENSION))
        .filter_map(|path| {
            let stem = path.file_stem()?.to_str()?.to_string();
            let text = std::fs::read_to_string(&path).ok()?;
            let front = Frontmatter::parse(&text);
            let name = front.get("name").map_or(stem, str::to_string);
            crate::skills::valid(&name).then(|| Agent {
                plugin: plugin.name.clone(),
                name,
                description: front.get("description").unwrap_or_default().to_string(),
                tools: front.get("tools").map(|list| {
                    list.split(',')
                        .map(str::trim)
                        .filter(|tool| !tool.is_empty())
                        .map(str::to_string)
                        .collect()
                }),
                instructions: plugin.expand(front.body.trim(), workspace),
            })
        })
        .collect()
}
