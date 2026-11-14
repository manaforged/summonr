use std::borrow::Cow;
use std::collections::HashSet;
use std::path::Path;

use summonr_core::dirs::{claude, summonr_home};

use crate::style::{Pick, Style};

mod imports;
mod rules;

const USER_TAG: &str = "user_context";
const PROJECT_TAG: &str = "project_context";
const AGENTS_MD: &str = "AGENTS.md";
const CLAUDE_MD: &str = "CLAUDE.md";
const LOCAL_MD: &str = "CLAUDE.local.md";

const PARAGRAPH: &str = "\n\n";
const CWD_OPEN: &str = "\n\n<cwd>";
const STYLE_OPEN: &str = "<output_style name=\"";
const STYLE_NAME_END: &str = "\">\n";
const STYLE_CLOSE: &str = "</output_style>";
const DEFAULT_NOTE: &str = "No output style is selected.";

const IMAGE_SECTION: (&str, &str) = ("<image_input>", "</image_input>");
const IMAGE_ON: &str = "<image_input>\nThe model accepts image input.\n</image_input>";
const IMAGE_OFF: &str = "<image_input>\nThe model does not accept image input. Do not attach images; describe them or ask the user for text instead.\n</image_input>";

#[derive(Clone, Copy, Default)]
pub struct Sources<'a> {
    pub trust: bool,
    pub skills: bool,
    /// The skill set the catalog shows; `None` shows every skill.
    pub set: Option<&'a crate::skills::Set>,
    pub images: bool,
    pub memory: bool,
    pub mcp: bool,
}

pub fn system_prompt(workspace: &Path, sources: Sources) -> String {
    compose(workspace, summonr_home().ok().as_deref(), sources)
}

/// The system prompt for `workspace` with the output style a new session starts with.
pub(crate) fn compose(workspace: &Path, home: Option<&Path>, sources: Sources) -> String {
    let pick = crate::style::active(workspace, sources.trust);
    styled(workspace, home, sources, pick.style())
}

/// The system prompt `compose` builds, with `style` as its output style.
pub(crate) fn styled(
    workspace: &Path,
    home: Option<&Path>,
    sources: Sources,
    style: Option<&Style>,
) -> String {
    let Sources {
        trust,
        skills,
        set,
        images,
        memory,
        mcp,
    } = sources;
    if let Ok(prompt) = std::env::var("SUMMONR_SYSTEM_PROMPT")
        && !prompt.trim().is_empty()
    {
        return prompt;
    }
    let mut parts = Vec::new();
    let replaced = home.is_some_and(|home| push_file(&mut parts, &home.join("SYSTEM.md")));
    if !replaced {
        parts.push(preamble(true));
    }
    if trust {
        push_file(&mut parts, &workspace.join("SYSTEM.md"));
        let user_home = summonr_core::dirs::user_home();
        instructions(&mut parts, workspace, user_home.as_deref());
        if let Some(user_home) = summonr_core::dirs::home_dir() {
            let state = crate::memory::repo_files(&user_home, workspace);
            parts.extend(crate::memory::sections(&state, workspace));
        }
        if let Some(home) = home.filter(|_| memory) {
            let files = crate::memory::files(home, workspace);
            parts.extend(crate::memory::sections(&files, workspace));
            parts.push(crate::memory::guidance(&files));
        }
    }
    if !(trust && memory) {
        parts.push(crate::memory::DISABLED.to_string());
    }
    if skills && let Some(catalog) = crate::skills::catalog(workspace, set) {
        parts.push(format!("<skills>\n{catalog}\n</skills>"));
    }
    if skills && let Some(catalog) = crate::plugins::agents::catalog(workspace) {
        parts.push(format!("<agents>\n{catalog}\n</agents>"));
    }
    if mcp && let Some(section) = crate::mcp::instructions(workspace) {
        parts.push(section);
    }
    parts.push(format!("<cwd>{}</cwd>", workspace.display()));
    parts.push(image_section(images).to_string());
    restyle(&parts.join(PARAGRAPH), style)
}

fn preamble(coding: bool) -> String {
    if coding {
        format!("{IDENTITY}{PARAGRAPH}<rules>\n{TOOL_RULES}\n{CODING_RULES}\n</rules>")
    } else {
        format!("<rules>\n{TOOL_RULES}\n</rules>")
    }
}

fn style_block(name: &str, text: &str) -> String {
    format!("{STYLE_OPEN}{name}{STYLE_NAME_END}{text}\n{STYLE_CLOSE}")
}

/// `system` with the output style section before `<cwd>` set to `style`, and the coding rules kept only when it asks; a prompt without `<cwd>` is unchanged.
pub(crate) fn restyle(system: &str, style: Option<&Style>) -> String {
    let Some(at) = system.rfind(CWD_OPEN) else {
        return system.to_string();
    };
    let (head, tail) = system.split_at(at);
    let head = match head.rfind(STYLE_OPEN) {
        Some(start) if head.ends_with(STYLE_CLOSE) => {
            let before = &head[..start];
            before.strip_suffix(PARAGRAPH).unwrap_or(before)
        }
        Some(_) | None => head,
    };
    let (full, bare) = (preamble(true), preamble(false));
    let lead = match style {
        Some(style) if !style.keep_coding => &bare,
        Some(_) | None => &full,
    };
    let rest = head
        .strip_prefix(full.as_str())
        .or_else(|| head.strip_prefix(bare.as_str()));
    let mut out = rest.map_or_else(|| head.to_string(), |rest| format!("{lead}{rest}"));
    if let Some(style) = style {
        out.push_str(PARAGRAPH);
        out.push_str(&style_block(&style.name, &style.instructions));
    }
    out.push_str(tail);
    out
}

/// The name of the output style `system` carries, if any.
pub(crate) fn style_name(system: &str) -> Option<&str> {
    let head = system[..system.rfind(CWD_OPEN)?].strip_suffix(STYLE_CLOSE)?;
    let start = head.rfind(STYLE_OPEN)? + STYLE_OPEN.len();
    head[start..]
        .split_once(STYLE_NAME_END)
        .map(|(name, _)| name)
}

/// The message that carries a mid-session switch to `pick` into the conversation.
pub(crate) fn style_note(pick: &Pick) -> String {
    match pick.style() {
        Some(style) => style_block(&style.name, &style.instructions),
        None => style_block(crate::style::DEFAULT, DEFAULT_NOTE),
    }
}

fn image_section(images: bool) -> &'static str {
    if images { IMAGE_ON } else { IMAGE_OFF }
}

/// Refresh only Summonr's trailing image guidance for the selected receiver.
pub(crate) fn model_system(system: &str, images: bool) -> Cow<'_, str> {
    let Some((prefix, _)) = system
        .strip_suffix(IMAGE_SECTION.1)
        .and_then(|body| body.rsplit_once(IMAGE_SECTION.0))
    else {
        return Cow::Borrowed(system);
    };
    let section = image_section(images);
    if system.ends_with(section) {
        return Cow::Borrowed(system);
    }
    Cow::Owned(format!("{prefix}{section}"))
}

pub(crate) struct Harness<'a> {
    pub(crate) mode: crate::WorkKind,
    pub(crate) checks: &'a [String],
    pub(crate) children: bool,
}

#[must_use]
pub(crate) fn harness_section(harness: &Harness<'_>) -> String {
    let &Harness {
        mode,
        checks,
        children,
    } = harness;
    let mut parts = vec![
        match mode {
            crate::WorkKind::Steps => "Harness state: work mode. Execute the selected steps in order.",
            crate::WorkKind::Goal => "Harness state: goal mode. The turn loop runs until the objective is met; a failed check starts another turn.",
            crate::WorkKind::Single => "Harness state: auto mode.",
        }
        .to_string(),
    ];
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
    if children && matches!(mode, crate::WorkKind::Steps | crate::WorkKind::Goal) {
        parts.push(
            "Name a child to edit this checkout. An unnamed implement child gets an isolated checkout that shares the parent build cache. Mail to the child's name is its next turn and stops its running command. Do not start a summonr process. The parent runs the completion check."
                .to_string(),
        );
    }
    if mode == crate::WorkKind::Single && parts.len() == 1 {
        return String::new();
    }
    parts.join("\n\n")
}

fn instructions(parts: &mut Vec<String>, workspace: &Path, home: Option<&Path>) {
    let mut load = Load {
        parts,
        sections: HashSet::new(),
        files: imports::Loader::new(home),
    };
    if let Some(home) = home {
        load.file(home, &home.join(".agents/AGENTS.md"), USER_TAG);
        load.file(home, &claude(home).join(CLAUDE_MD), USER_TAG);
        load.rules(home, home, USER_TAG);
        load.file(home, &home.join(".config/devin/AGENTS.md"), USER_TAG);
    }
    let root = std::path::absolute(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    let mut above: Vec<&Path> = root.ancestors().skip(1).collect();
    above.reverse();
    for dir in above {
        load.claude_files(workspace, dir);
        load.file(workspace, &dir.join(LOCAL_MD), PROJECT_TAG);
    }
    load.file(workspace, &workspace.join(AGENTS_MD), PROJECT_TAG);
    load.claude_files(workspace, workspace);
    load.rules(workspace, workspace, PROJECT_TAG);
    load.file(workspace, &workspace.join(LOCAL_MD), PROJECT_TAG);
}

struct Load<'a> {
    parts: &'a mut Vec<String>,
    sections: HashSet<u64>,
    files: imports::Loader<'a>,
}

impl Load<'_> {
    fn file(&mut self, base: &Path, path: &Path, tag: &str) {
        if self.files.claim(path)
            && let Some(text) = self.files.expand(path)
        {
            self.push(base, path, tag, &text);
        }
    }

    fn claude_files(&mut self, base: &Path, dir: &Path) {
        self.file(base, &dir.join(CLAUDE_MD), PROJECT_TAG);
        self.file(base, &claude(dir).join(CLAUDE_MD), PROJECT_TAG);
    }

    fn rules(&mut self, base: &Path, root: &Path, tag: &str) {
        for (path, body) in rules::launch(root) {
            if self.files.claim(&path) && !body.trim().is_empty() {
                self.push(base, &path, tag, &body);
            }
        }
    }

    fn push(&mut self, base: &Path, path: &Path, tag: &str, text: &str) {
        if let Some(text) = dedup_sections(text, &mut self.sections) {
            let label = label(path, base);
            self.parts
                .push(format!("<{tag} path=\"{label}\">\n{text}\n</{tag}>"));
        }
    }
}

fn label(path: &Path, base: &Path) -> String {
    path.strip_prefix(base).map_or_else(
        |_| path.display().to_string(),
        |rel| {
            rel.components()
                .map(|part| part.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/")
        },
    )
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

fn dedup_sections(text: &str, seen: &mut HashSet<u64>) -> Option<String> {
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
            let mut hasher = summonr_core::text::Fnv64::default();
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

const IDENTITY: &str =
    "You are an expert coding assistant running inside summonr, a terminal coding harness.";
const TOOL_RULES: &str = "- To add a panel, chart or dashboard tab to the summonr desktop app, read `summonr panel --help`, write the file it describes, and run `summonr panel check NAME`.
- Use search to find text in files or files by name, anywhere on disk; read for files and directories; run_command for builds, tests, and git
- Unsure of a tool's options? call it with op=help for the full reference
- A tool missing from the schemas is an off or empty module: web (search, fetch), mail (agents, send, inbox, board), MCP, skills, LSP, jobs, child tasks. Name the step (`summonr module list`, `summonr mcp add NAME -- CMD`, `~/.summonr/skills/<name>/SKILL.md`) instead of hunting the harness
- Batch: one edit call with an edits array for several changes in a file; independent calls in one response run in order
- write is for new files and complete rewrites";
const CODING_RULES: &str = "- Be concise in your responses
- Finish the whole request before you reply, unless only the user can decide the next step";

#[cfg(test)]
#[path = "prompt_tests.rs"]
mod tests;
