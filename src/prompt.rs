use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

pub fn mage_home() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("MAGE_HOME") {
        if path.is_empty() {
            return Err(Error::Config("MAGE_HOME is empty".to_string()));
        }
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| {
            Error::Config("set MAGE_HOME because the home directory is unknown".to_string())
        })?;
    Ok(PathBuf::from(home).join(".mage"))
}

pub fn system_prompt(workspace: &Path) -> String {
    compose(workspace, mage_home().ok().as_deref(), false)
}

pub fn system_prompt_with(workspace: &Path, trust: bool) -> String {
    compose(workspace, mage_home().ok().as_deref(), trust)
}

pub(crate) fn compose(workspace: &Path, home: Option<&Path>, trust: bool) -> String {
    if let Ok(prompt) = std::env::var("MAGE_SYSTEM_PROMPT")
        && !prompt.trim().is_empty()
    {
        return prompt;
    }
    let mut parts = Vec::new();
    if let Some(home) = home {
        push_file(&mut parts, &home.join("SYSTEM.md"));
    }
    push_file(&mut parts, &workspace.join("SYSTEM.md"));
    if trust {
        push_file(&mut parts, &workspace.join("AGENTS.md"));
    }
    if parts.is_empty() {
        parts.push(default_prompt());
    }
    if let Some(catalog) = crate::skills::catalog(workspace) {
        parts.push(catalog);
    }
    parts.join("\n\n")
}

#[must_use]
pub fn harness_section(plan: bool, checks: &[String]) -> String {
    let mut parts = vec![if plan {
        "Harness state: plan mode is active. Tools are read-only. Explore, analyze, and report; propose concrete changes as diffs in your answer.".to_string()
    } else {
        "Harness state: implementation mode is active.".to_string()
    }];
    if checks.is_empty() {
        parts.push("Harness: no completion check is configured.".to_string());
    } else {
        let list = checks
            .iter()
            .map(|check| format!("`{check}`"))
            .collect::<Vec<_>>()
            .join(" then ");
        parts.push(format!(
            "Harness: before Work can complete, Summoner runs {list} as the completion check."
        ));
    }
    parts.join("\n\n")
}

fn push_file(parts: &mut Vec<String>, path: &Path) {
    if let Ok(text) = std::fs::read_to_string(path)
        && !text.trim().is_empty()
    {
        parts.push(text);
    }
}

fn default_prompt() -> String {
    "Complete the user's engineering task in the current workspace. Keep reasoning brief. \
     Inspect only the relevant code and tests, make the smallest complete edit, and run the \
     narrowest useful check. Batch independent calls. Do not repeat a read, search, or check \
     without new evidence. Stop when the task is proved. Do not claim work that you did not do."
        .to_string()
}
