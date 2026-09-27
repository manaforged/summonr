use super::*;

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
    let cutoff = now().saturating_sub(STALE_SECS);
    let sweep = now().saturating_sub(SWEEP_SECS);
    let mut roster = Vec::new();
    for entry in std::fs::read_dir(dir.join("agents"))
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
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

pub(super) fn targets(agents: &[Agent], from: &str, words: &[String]) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
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
        let found: Vec<String> = if matched.is_empty() && word != "all" {
            vec![word.clone()]
        } else {
            matched.iter().map(|agent| agent.id.clone()).collect()
        };
        for id in found {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids
}
