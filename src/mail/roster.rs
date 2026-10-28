use std::collections::HashSet;

use super::*;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Agent {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    pub model: String,
    pub cwd: String,
    pub pid: u32,
    pub status: Presence,
    pub last_seen: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
}

impl Agent {
    pub(crate) fn label(&self) -> &str {
        if self.name.is_empty() {
            &self.id
        } else {
            &self.name
        }
    }

    pub(crate) fn answers_to(&self, word: &str) -> bool {
        !word.is_empty() && (self.name == word || self.id == word)
    }
}

pub(crate) fn live(dir: &Path) -> Vec<Agent> {
    let cutoff = now().saturating_sub(crate::limits::limits().mail_stale_secs);
    let sweep = now().saturating_sub(crate::limits::limits().mail_sweep_secs);
    let mut roster = Vec::new();
    for entry in summonr_core::walk::children(&dir.join("agents"))
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
    {
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let Ok(agent) = serde_json::from_str::<Agent>(&text) else {
            continue;
        };
        if agent.last_seen < sweep {
            drop(std::fs::remove_file(entry.path()));
        } else if agent.last_seen >= cutoff {
            roster.push(agent);
        }
    }
    roster.sort_by(|a, b| a.id.cmp(&b.id));
    roster
}

pub(crate) fn roster(agents: &[Agent], me: &str) -> String {
    if agents.is_empty() {
        return "no live agents".to_string();
    }
    agents
        .iter()
        .map(|agent| {
            let you = if agent.id == me { " (you)" } else { "" };
            format!(
                "@{} · {} · {} · {} · {}{you}",
                agent.label(),
                agent.kind,
                agent.model,
                agent.cwd,
                agent.status
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn unique_name(agents: &[Agent], id: &str, hint: Option<&str>, cwd: &Path) -> String {
    let taken: Vec<&str> = agents
        .iter()
        .filter(|agent| agent.id != id)
        .map(|agent| agent.name.as_str())
        .collect();
    let hint = hint.map(clean_id).filter(|hint| !hint.is_empty());
    if let Some(hint) = hint.as_deref()
        && !taken.contains(&hint)
    {
        return hint.to_string();
    }
    let (base, first) = match hint {
        Some(hint) => (hint, 2),
        None => (
            cwd.file_name()
                .map(|name| clean_id(&name.to_string_lossy()))
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| "session".to_string()),
            1,
        ),
    };
    (first..)
        .map(|number| format!("{base}-{number}"))
        .find(|name| !taken.contains(&name.as_str()))
        .unwrap_or(base)
}

pub(super) fn targets(agents: &[Agent], from: &str, words: &[String]) -> Result<Vec<String>> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    for word in words {
        let matched: Vec<&Agent> = agents
            .iter()
            .filter(|agent| {
                if word == "all" {
                    agent.id != from
                } else {
                    agent.answers_to(word)
                }
            })
            .collect();
        if matched.is_empty() && word != "all" {
            return Err(Error::Config(format!("no session named {word}")));
        }
        for agent in matched {
            if seen.insert(agent.id.as_str()) {
                ids.push(agent.id.clone());
            }
        }
    }
    Ok(ids)
}
