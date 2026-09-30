use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

pub fn summonr_home() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("SUMMONR_HOME") {
        if path.is_empty() {
            return Err(Error::Config("SUMMONR_HOME is empty".to_string()));
        }
        return Ok(PathBuf::from(path));
    }
    let home = user_home().ok_or_else(|| {
        Error::Config("set SUMMONR_HOME because the home directory is unknown".to_string())
    })?;
    let dir = home.join(".summonr");
    let legacy = dir.with_file_name(".mage");
    if !dir.exists() && legacy.is_dir() {
        drop(std::fs::rename(&legacy, &dir));
    }
    if !dir.is_dir() {
        std::fs::create_dir_all(&dir)?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&dir)?.permissions().mode() & 0o777;
        if mode != 0o700 {
            std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(dir)
}

#[derive(Clone, Copy, Default)]
pub struct Sources {
    pub trust: bool,
    pub skills: bool,
    pub images: bool,
}

pub fn system_prompt(workspace: &Path, sources: Sources) -> String {
    compose(workspace, summonr_home().ok().as_deref(), sources)
}

pub(crate) fn compose(workspace: &Path, home: Option<&Path>, sources: Sources) -> String {
    let Sources {
        trust,
        skills,
        images,
    } = sources;
    if let Ok(prompt) = std::env::var("SUMMONR_SYSTEM_PROMPT")
        && !prompt.trim().is_empty()
    {
        return prompt;
    }
    let mut preamble = Vec::new();
    if let Some(home) = home {
        push_file(&mut preamble, &home.join("SYSTEM.md"));
    }
    if trust {
        push_file(&mut preamble, &workspace.join("SYSTEM.md"));
    }
    let mut parts = if preamble.is_empty() {
        vec![DEFAULT.to_string()]
    } else {
        preamble
    };
    if trust {
        instructions(&mut parts, workspace);
    }
    if !images {
        parts.push(
            "The model does not accept image input. Do not attach images; describe them or ask the user for text instead."
                .to_string(),
        );
    }
    if skills && let Some(catalog) = crate::skills::catalog(workspace) {
        parts.push(format!("<skills>\n{catalog}\n</skills>"));
    }
    parts.push(format!("<cwd>{}</cwd>", workspace.display()));
    parts.join("\n\n")
}

pub(crate) struct Harness<'a> {
    pub(crate) mode: crate::RunMode,
    pub(crate) checks: &'a [String],
    pub(crate) sandbox: Option<crate::Network>,
    pub(crate) children: bool,
}

#[must_use]
pub(crate) fn harness_section(harness: &Harness<'_>) -> String {
    let &Harness {
        mode,
        checks,
        sandbox,
        children,
    } = harness;
    let mut parts = vec![
        match mode {
            crate::RunMode::Plan => "Harness state: plan mode. Read-only. Inspect, then propose diffs in the answer.",
            crate::RunMode::Work => "Harness state: work mode. Execute the accepted plan.",
            crate::RunMode::Goal => "Harness state: goal mode. The turn loop runs until the objective is met; a failed check starts another turn.",
            crate::RunMode::Auto => "Harness state: auto mode.",
        }
        .to_string(),
    ];
    let auto = mode == crate::RunMode::Auto;
    if let Some(network) = sandbox {
        parts.push(if network == crate::Network::Allowed {
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
    if children && matches!(mode, crate::RunMode::Work | crate::RunMode::Goal) {
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

fn user_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn instructions(parts: &mut Vec<String>, workspace: &Path) {
    let mut seen = std::collections::HashSet::new();
    let mut sections = std::collections::HashSet::new();
    if let Some(home) = user_home() {
        for rel in [
            ".agents/AGENTS.md",
            ".claude/CLAUDE.md",
            ".config/devin/AGENTS.md",
        ] {
            context_file(parts, &mut seen, &mut sections, &home, rel, "user_context");
        }
    }
    for name in ["AGENTS.md", "CLAUDE.md"] {
        context_file(
            parts,
            &mut seen,
            &mut sections,
            workspace,
            name,
            "project_context",
        );
    }
}

fn context_file(
    parts: &mut Vec<String>,
    seen: &mut std::collections::HashSet<PathBuf>,
    sections: &mut std::collections::HashSet<u64>,
    dir: &Path,
    rel: &str,
    tag: &str,
) {
    let path = dir.join(rel);
    if !seen.insert(path.canonicalize().unwrap_or_else(|_| path.clone())) {
        return;
    }
    let mut context = Vec::new();
    push_file(&mut context, &path);
    if let Some(text) = context.pop()
        && let Some(text) = dedup_sections(&text, sections)
    {
        parts.push(format!("<{tag} path=\"{rel}\">\n{text}\n</{tag}>"));
    }
}

fn push_file(parts: &mut Vec<String>, path: &Path) {
    if let Ok(text) = std::fs::read_to_string(path)
        && !text.trim().is_empty()
    {
        parts.push(text);
    }
}

fn dedup_sections(text: &str, seen: &mut std::collections::HashSet<u64>) -> Option<String> {
    let mut kept: Vec<String> = Vec::new();
    let mut section = String::new();
    let mut flush = |section: &mut String| {
        let trimmed = section.trim();
        if trimmed.is_empty() {
            return;
        }
        let normalized: String = trimmed
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        let hash = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            normalized.hash(&mut hasher);
            hasher.finish()
        };
        if normalized.len() < 40 || seen.insert(hash) {
            kept.push(trimmed.to_string());
        }
        section.clear();
    };
    for line in text.lines() {
        if line.starts_with('#') {
            flush(&mut section);
        }
        section.push_str(line);
        section.push('\n');
    }
    flush(&mut section);
    (!kept.is_empty()).then(|| kept.join("\n\n"))
}

const DEFAULT: &str = "You are a coding assistant running inside summonr, a terminal coding harness. You read files, run commands, edit code, and write files.

<rules>
- Use read for files, directories, and content search (honors ignore rules); run_command covers the rest — builds, tests, git, ls, grep
- Unsure of a tool's options? call it with op=help for the full reference
- Batch: one edit call with an edits array for several changes in a file; independent calls in one response run in order
- write is for new files and complete rewrites
- Be concise in your responses
</rules>";

#[cfg(test)]
#[path = "prompt_tests.rs"]
mod tests;
