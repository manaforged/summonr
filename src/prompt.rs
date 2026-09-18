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
        return format!("{prompt}\n\n{PERSISTENCE}");
    }
    let mut preamble = Vec::new();
    if let Some(home) = home {
        push_file(&mut preamble, &home.join("SYSTEM.md"));
    }
    push_file(&mut preamble, &workspace.join("SYSTEM.md"));
    let mut parts = if preamble.is_empty() {
        vec![DEFAULT.to_string()]
    } else {
        preamble
    };
    parts.push(PERSISTENCE.to_string());
    if trust {
        let mut context = Vec::new();
        push_file(&mut context, &workspace.join("AGENTS.md"));
        if let Some(text) = context.pop() {
            parts.push(format!(
                "<project_context path=\"AGENTS.md\">\n{}\n</project_context>",
                text.trim()
            ));
        }
    }
    if skills && let Some(catalog) = crate::skills::catalog(workspace) {
        parts.push(format!("<skills>\n{catalog}\n</skills>"));
    }
    parts.push(format!("<cwd>{}</cwd>", workspace.display()));
    parts.join("\n\n")
}

#[must_use]
pub fn harness_section(
    mode: crate::RunMode,
    checks: &[String],
    network: bool,
    sandboxed: bool,
) -> String {
    let mut parts = vec![
        match mode {
            crate::RunMode::Plan => "Harness state: plan mode. Read-only. Inspect, then propose diffs in the answer.",
            crate::RunMode::Work => "Harness state: work mode. Execute the accepted plan. Keep the itemized list.",
            crate::RunMode::Goal => "Harness state: goal mode. Keep going until the objective is met. A failed check continues the loop.",
            crate::RunMode::Auto => "Harness state: auto mode.",
        }
        .to_string(),
    ];
    let auto = mode == crate::RunMode::Auto;
    if sandboxed {
        parts.push(if network {
            "Shell commands run in the OS sandbox with network allowed. Writes stay under the workspace and [sandbox] write roots."
        } else {
            "Shell commands run in the OS sandbox with network denied. Writes stay under the workspace and [sandbox] write roots."
        }
        .to_string());
    }
    #[cfg(windows)]
    parts.push(
        "Shell commands use Windows PowerShell. Use PowerShell syntax and native Windows programs."
            .to_string(),
    );
    if !checks.is_empty() {
        let list = checks
            .iter()
            .map(|check| format!("`{check}`"))
            .collect::<Vec<_>>()
            .join(" then ");
        parts.push(format!(
            "Harness: before Work can complete, Summonr runs {list} as the completion check."
        ));
    }
    if matches!(mode, crate::RunMode::Work | crate::RunMode::Goal) {
        parts.push(
            "Spawn a child only for disjoint paths. The parent runs the completion check."
                .to_string(),
        );
    }
    if auto && parts.len() == 1 {
        return String::new();
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

const PERSISTENCE: &str = "<persistence>
- Continue the authorized task until it is complete. Uncertainty about finishing within this session is not a reason to stop.
- Do not reduce scope or hand unfinished work back because the task seems too large for the remaining context.
- Summonr automatically compacts context. Keep progress, decisions, and remaining work clear so you can continue the same task after compaction.
- After compaction, resume from the preserved state. Compaction does not complete or cancel the task.
- Respect user cancellation, explicit limits, and permission boundaries. If a concrete blocker requires user input, ask and continue independent work.
</persistence>";

const DEFAULT: &str = "You are a coding assistant running inside summonr, a terminal coding harness. You help the user by reading files, running commands, editing code, and writing new files.

<tools>
- read: read a file, list a directory, or search file contents
- run_command: run a shell command in the workspace
- edit: change lines in an existing file using the tags that read returns
- write: create or replace a file
Other tools may be available depending on configuration.
</tools>

<rules>
- Use run_command for ls, rg, find, builds, and tests
- When changing several places in one file, make one edit call with an edits array instead of several edit calls
- Use write only for new files or complete rewrites
- Put tool calls that do not need to see an earlier call's result in the same response; they run in order, so you can write a script and run it in one step
- Make the smallest change that does the job: reuse existing code, and add no new layers, abstractions, config, or dependencies the task does not need
- Be concise in your responses
- Show file paths clearly when working with files
</rules>";
