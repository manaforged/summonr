use std::path::{Path, PathBuf};

pub(crate) const FILE: &str = "MEMORY.md";
const DIR: &str = "memory";
const GIT_DIR: &str = ".git";
const REPORT_END: &str = "Ask me to remember something, or edit a file above. `memory = false` under [modules] turns memory off.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    User,
    Project,
}

impl Scope {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryFile {
    pub scope: Scope,
    pub path: PathBuf,
    pub bytes: u64,
}

pub fn files(home: &Path, workspace: &Path) -> Vec<MemoryFile> {
    let root = home.join(DIR);
    [
        (Scope::User, root.join(FILE)),
        (Scope::Project, root.join(key(workspace)).join(FILE)),
    ]
    .into_iter()
    .map(|(scope, path)| MemoryFile {
        scope,
        bytes: std::fs::metadata(&path).map_or(0, |meta| meta.len()),
        path,
    })
    .collect()
}

pub(crate) fn sections(files: &[MemoryFile], workspace: &Path) -> Vec<String> {
    let cap = crate::limits::limits().memory_bytes;
    files
        .iter()
        .filter_map(|file| {
            let raw = std::fs::read_to_string(&file.path).ok()?;
            let text = clean(&raw, workspace);
            let text = text.trim();
            if text.is_empty() {
                return None;
            }
            let body = if text.len() > cap {
                let mut end = cap;
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                format!(
                    "{}\n[memory truncated at {cap} bytes; prune this file]",
                    &text[..end]
                )
            } else {
                text.to_string()
            };
            Some(format!(
                "<memory scope=\"{}\" path=\"{}\">\n{body}\n</memory>",
                file.scope.label(),
                file.path.display()
            ))
        })
        .collect()
}

pub fn report(files: &[MemoryFile]) -> String {
    let cap = crate::limits::limits().memory_bytes;
    let mut text = String::new();
    for file in files {
        let name = format!("{} memory", file.scope.label());
        text.push_str(&format!(
            "{name:<16} {}\n  {}\n",
            crate::config::tilde(&file.path),
            status(file, cap)
        ));
    }
    text.push_str(REPORT_END);
    text
}

fn status(file: &MemoryFile, cap: usize) -> String {
    if file.bytes == 0 {
        return "empty".to_string();
    }
    let entries = std::fs::read_to_string(&file.path)
        .unwrap_or_default()
        .lines()
        .map(str::trim_start)
        .filter(|line| line.starts_with("- ") || line.starts_with(|c: char| c.is_ascii_digit()))
        .count();
    format!(
        "{} of {cap} bytes, {}",
        file.bytes,
        crate::text::plural(entries, "entry")
    )
}

pub(crate) fn guidance(files: &[MemoryFile]) -> String {
    let path = |scope: Scope| {
        files
            .iter()
            .find(|file| file.scope == scope)
            .map(|file| file.path.display().to_string())
            .unwrap_or_default()
    };
    format!(
        "Memory: project memory is {project} and user memory is {user}. Save a durable fact to project memory with edit or write (write creates a missing file): a user correction, a stable preference, a non-obvious project fact, or a pointer to an outside resource. Put cross-project preferences in user memory. Never save task state or what git already records. Start each entry with its date (YYYY-MM-DD). Prune the file to keep it under {cap} bytes. After saving, tell the user in one line what you saved and where. Check a remembered fact against the code before you act on it; delete it when wrong.",
        project = path(Scope::Project),
        user = path(Scope::User),
        cap = crate::limits::limits().memory_bytes,
    )
}

fn key(workspace: &Path) -> String {
    let root = crate::diff::git_env(
        workspace,
        &[],
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .ok()
    .filter(|dir| !dir.is_empty())
    .map(PathBuf::from)
    .map(|common| {
        if common.file_name().is_some_and(|name| name == GIT_DIR) {
            common.parent().map_or(common.clone(), Path::to_path_buf)
        } else {
            common
        }
    })
    .unwrap_or_else(|| {
        workspace
            .canonicalize()
            .unwrap_or_else(|_| workspace.to_path_buf())
    });
    root.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn clean(text: &str, workspace: &Path) -> String {
    let visible: String = text.chars().filter(|c| !invisible(*c)).collect();
    crate::secret::shield::scrub_found(&visible, workspace)
}

fn invisible(c: char) -> bool {
    matches!(
        c,
        '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
            | '\u{E0000}'..='\u{E007F}'
    )
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
