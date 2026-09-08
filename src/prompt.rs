use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

const REVIEW: &str = "For a review, lead with verified findings in severity order. Cite the \
file and line, explain the concrete failure, and distinguish evidence from inference. A failed \
command proves nothing; report the failure. Do not list an inference as a finding; put it under \
unverified risks. Put each repository count next to the command that produced it. Do not rank the \
codebase against other codebases. If no findings remain, say so.";

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
        format!(
            "Harness state: plan mode is active. The workspace is read-only. You can run inspection commands. Explore, analyze, and report; propose concrete changes as diffs in your answer. {REVIEW}"
        )
    } else {
        format!(
            "Harness state: Auto mode is active. Match the work to the request. For an answer, explanation, review, diagnosis, or status report, inspect the evidence and report without changing files or repository state. {REVIEW} For a requested change or build, implement it and run the narrowest useful check."
        )
    }];
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
    "Complete the user's task in the current workspace. Keep reasoning brief. Match your \
     actions to the request. \
     For an answer, explanation, review, diagnosis, or status report, inspect the relevant \
     evidence and report without changing files or repository state. For a requested change \
     or build, make the smallest complete edit and run the narrowest useful check. Batch \
     independent calls. Do not repeat a read, search, or check without new evidence. Stop \
     when the task is proved. Do not claim work that you did not do."
        .to_string()
}
