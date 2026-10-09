use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

static READY: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

pub fn summonr_home() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("SUMMONR_HOME") {
        if path.is_empty() {
            return Err(Error::Config("SUMMONR_HOME is empty".to_string()));
        }
        return Ok(PathBuf::from(path));
    }
    let home = crate::config::home_dir().ok_or_else(|| {
        Error::Config("set SUMMONR_HOME because the home directory is unknown".to_string())
    })?;
    let dir = home.join(".summonr");
    if READY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        == Some(&dir)
    {
        return Ok(dir);
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
    *READY
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(dir.clone());
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
    let mut parts = Vec::new();
    let replaced = home.is_some_and(|home| push_file(&mut parts, &home.join("SYSTEM.md")));
    if !replaced {
        parts.push(DEFAULT.to_string());
    }
    if trust {
        push_file(&mut parts, &workspace.join("SYSTEM.md"));
    }
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
    if skills && let Some(catalog) = crate::plugins::agents::catalog(workspace) {
        parts.push(format!("<agents>\n{catalog}\n</agents>"));
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
            crate::RunMode::Work => "Harness state: work mode. Execute the selected steps in order.",
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
            "Harness: before Work can complete, summonr runs {list} as the completion check."
        ));
    }
    if children && matches!(mode, crate::RunMode::Work | crate::RunMode::Goal) {
        parts.push(
            "Spawn a child only for disjoint paths. An implement child gets an isolated checkout that shares the parent build cache. The parent runs the completion check."
                .to_string(),
        );
    }
    if auto && parts.len() == 1 {
        return String::new();
    }
    parts.join("\n\n")
}

fn instructions(parts: &mut Vec<String>, workspace: &Path) {
    let mut seen = std::collections::HashSet::new();
    let mut sections = std::collections::HashSet::new();
    if let Some(home) = crate::config::home_dir() {
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
    if let Some(text) = expanded(&path, seen)
        && let Some(text) = dedup_sections(&text, sections)
    {
        parts.push(format!("<{tag} path=\"{rel}\">\n{text}\n</{tag}>"));
    }
}

fn expanded(path: &Path, seen: &mut std::collections::HashSet<PathBuf>) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = Vec::new();
    for line in text.lines() {
        let Some(target) = include_target(line) else {
            lines.push(line.to_string());
            continue;
        };
        let dir = path.parent().unwrap_or(path).to_path_buf();
        let target = dir.join(target);
        if !seen.insert(target.canonicalize().unwrap_or_else(|_| target.clone())) {
            continue;
        }
        match expanded(&target, seen) {
            Some(text) => lines.push(text),
            None => lines.push(line.to_string()),
        }
    }
    let text = lines.join("\n");
    (!text.trim().is_empty()).then_some(text)
}

fn include_target(line: &str) -> Option<&str> {
    let rest = line.trim_start().strip_prefix('@')?;
    let token = rest.split_whitespace().next()?;
    (!token.is_empty()).then_some(token)
}

fn push_file(parts: &mut Vec<String>, path: &Path) -> bool {
    if let Ok(text) = std::fs::read_to_string(path)
        && !text.trim().is_empty()
    {
        parts.push(text);
        return true;
    }
    false
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
            use std::hash::Hasher;
            let mut hasher = crate::text::Fnv64::default();
            hasher.write(normalized.as_bytes());
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

const DEFAULT: &str = "You are an expert coding assistant running inside summonr, a terminal coding harness. You read files, run commands, edit code, and write files.

<rules>
- Use search to find text in files or files by name, anywhere on disk; read for files and directories; run_command for builds, tests, and git
- Unsure of read, edit, or run_command options? call it with op=help for the full reference
- Tool schemas are the full callable surface; modules add more when on: web (search, fetch), mail (agents, send, inbox, board), MCP servers, skills, LSP, background jobs, child tasks
- A listed tool you cannot see is a module that is off (`summonr modules` lists them; ask the user to enable) or on with nothing installed: skills need a SKILL.md under `~/.summonr/skills/<name>/`, MCP needs `summonr mcp add NAME -- CMD`, web needs a provider. Name the exact step instead of hunting the harness
- Batch: one edit call with an edits array for several changes in a file; independent calls in one response run in order
- write is for new files and complete rewrites
- Be concise in your responses
</rules>";

#[cfg(test)]
#[path = "prompt_tests.rs"]
mod tests;
