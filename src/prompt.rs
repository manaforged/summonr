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
    compose(workspace, mage_home().ok().as_deref(), false, true)
}

pub fn system_prompt_with(workspace: &Path, trust: bool, skills: bool) -> String {
    compose(workspace, mage_home().ok().as_deref(), trust, skills)
}

pub(crate) fn compose(workspace: &Path, home: Option<&Path>, trust: bool, skills: bool) -> String {
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
    if skills && let Some(catalog) = crate::skills::catalog(workspace) {
        parts.push(catalog);
    }
    parts.join("\n\n")
}

#[must_use]
pub fn harness_section(plan: bool, checks: &[String]) -> String {
    let mut parts = vec![if plan {
        "Harness state: plan mode. Read-only. Inspect, then propose diffs in the answer."
            .to_string()
    } else {
        "Harness state: auto mode. Inspect unless they asked for a change, then implement it."
            .to_string()
    }];
    parts.push(
        "Search with read (pattern). A shell grep -r or find from the repository root walks target/ and node_modules/."
            .to_string(),
    );
    parts.push("Shell commands run in the OS sandbox with network denied. Writes are allowed under the workspace and [sandbox] write roots. Auto mode grants a write outside those roots for this session and retries. /approve on asks first. Standing roots: [sandbox] write in ~/.mage/config.toml. Network: /net on or /net once.".to_string());
    #[cfg(windows)]
    parts.push("Shell commands use Windows PowerShell. Use PowerShell syntax and native Windows programs. The sandbox uses a PowerShell drive rooted at the workspace. Use relative paths, or $PWD.ProviderPath when a native program needs the filesystem path. Git Bash and other MSYS programs cannot run inside the Windows sandbox.".to_string());
    if checks.is_empty() {
        parts.push("Harness: no completion check is configured.".to_string());
    } else {
        let list = checks
            .iter()
            .map(|check| format!("`{check}`"))
            .collect::<Vec<_>>()
            .join(" then ");
        parts.push(format!(
            "Harness: before Work can complete, Summonr runs {list} as the completion check."
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
    "Complete the user's task in the current workspace. Inspect and report unless they asked for a change."
        .to_string()
}
