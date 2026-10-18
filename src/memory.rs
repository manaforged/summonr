use std::path::{Path, PathBuf};

pub(crate) const FILE: &str = "MEMORY.md";
const DIR: &str = "memory";
const KEY_NAME_CHARS: usize = 40;
const FIRST_COMMIT: &str = "first-commit";
const OVER: &str = "; only the newest part loads; ask me to read and tidy it";
const REPORT_END: &str = "Ask me to remember something, or edit a file above; changes to these files apply to new sessions. `memory = false` under [modules] turns memory off.";
pub(crate) const DISABLED: &str = "Memory is off in this session, so nothing you save persists; if the user asks you to remember something, say so instead of writing a file.";
pub const OFF: &str = "memory is off: `memory = false` under [modules] in ~/.summonr/config.toml. Turn it on in /settings or with `memory = true`; it applies to new sessions. Sessions started with --no-trust also load no memory.\n";

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
        (Scope::Project, root.join(key(&root, workspace)).join(FILE)),
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
            let raw = read_tail(&file.path, cap)?;
            let text = clean(&raw, workspace);
            let text = text.trim();
            if text.is_empty() {
                return None;
            }
            let total = usize::try_from(file.bytes)
                .unwrap_or(usize::MAX)
                .max(text.len());
            Some(format!(
                "<memory scope=\"{}\" path=\"{}\">\n{}\n</memory>",
                file.scope.label(),
                file.path.display(),
                excerpt(text, total, cap, &file.path)
            ))
        })
        .collect()
}

fn excerpt(text: &str, total: usize, cap: usize, path: &Path) -> String {
    if total <= cap && text.len() <= cap {
        return text.to_string();
    }
    let room = omitted(total, path).len();
    let mut start = text.len().saturating_sub(cap.saturating_sub(room));
    while !text.is_char_boundary(start) {
        start += 1;
    }
    if start > 0 && !text[..start].ends_with('\n') {
        start = text[start..]
            .find('\n')
            .map_or(start, |line| start + line + 1);
    }
    let kept = &text[start..];
    format!("{}{kept}", omitted(total.saturating_sub(kept.len()), path))
}

fn omitted(older: usize, path: &Path) -> String {
    format!(
        "[{older} older bytes of this file are not shown. Read {} in full before you edit it; never rewrite it from this excerpt.]\n",
        path.display()
    )
}

fn read_tail(path: &Path, cap: usize) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let limit = u64::try_from(cap.saturating_mul(4)).unwrap_or(u64::MAX);
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(limit))).ok()?;
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes).ok()?;
    Some(String::from_utf8_lossy(&bytes).into_owned())
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
    text + REPORT_END
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
    let over = if file.bytes > u64::try_from(cap).unwrap_or(u64::MAX) {
        OVER
    } else {
        ""
    };
    format!(
        "{} of {cap} bytes, {}{over}",
        file.bytes,
        crate::text::plural(entries, "entry")
    )
}

pub(crate) fn guidance(files: &[MemoryFile]) -> String {
    let path = |scope: Scope| {
        files
            .iter()
            .find(|file| file.scope == scope)
            .map(|file| {
                let empty = if file.bytes == 0 {
                    " (empty, nothing to read)"
                } else {
                    ""
                };
                format!("{}{empty}", file.path.display())
            })
            .unwrap_or_default()
    };
    format!(
        "Memory: project memory is {project} and user memory is {user}. Save a durable fact to project memory with edit or write (write creates a missing file): a user correction, a stable preference, a non-obvious project fact, or a pointer to an outside resource. Put cross-project preferences in user memory. Never save task state or what git already records. Start each entry with its date (YYYY-MM-DD); today is {today}. Read the file before you change it; use edit for one entry, never rewrite the whole file from memory. Keep the file under {cap} bytes. Never delete entries the user did not ask you to remove. When the file is over the cap, ask the user which entries to drop before pruning; never prune with a shell command. After saving, tell the user in one line what you saved and where. Check a remembered fact against the code before you act on it; delete it when wrong.",
        project = path(Scope::Project),
        user = path(Scope::User),
        cap = crate::limits::limits().memory_bytes,
        today = jiff::Zoned::now().date(),
    )
}

fn key(memory: &Path, workspace: &Path) -> String {
    let layout = crate::diff::layout(workspace);
    let root = root(workspace, layout.as_ref());
    let identity = first_commit(workspace, layout.as_ref())
        .unwrap_or_else(|| root.to_string_lossy().into_owned());
    let name: String = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .take(KEY_NAME_CHARS)
        .collect();
    let hash = crate::text::fnv64(identity.as_bytes());
    existing(memory, &hash).unwrap_or_else(|| format!("{name}-{hash}"))
}

fn existing(memory: &Path, hash: &str) -> Option<String> {
    let suffix = format!("-{hash}");
    std::fs::read_dir(memory)
        .ok()?
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .find(|name| name.ends_with(&suffix))
}

fn first_commit(workspace: &Path, layout: Option<&crate::diff::Layout>) -> Option<String> {
    if !crate::diff::inside_git(workspace) {
        return None;
    }
    let saved = layout.map(|layout| {
        layout
            .common
            .join(crate::diff::STATE_DIR)
            .join(FIRST_COMMIT)
    });
    if let Some(id) = saved
        .as_deref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| text.trim().to_string())
        .filter(|id| !id.is_empty())
    {
        return Some(id);
    }
    let id = crate::diff::git_env(workspace, &[], &["rev-list", "--max-parents=0", "HEAD"])
        .ok()
        .and_then(|ids| ids.lines().next().map(str::trim).map(str::to_string))
        .filter(|id| !id.is_empty())?;
    if let Some(path) = saved {
        drop(crate::durable::store::write(
            &path,
            format!("{id}\n"),
            crate::durable::store::Durability::Renamed,
        ));
    }
    Some(id)
}

fn root(workspace: &Path, layout: Option<&crate::diff::Layout>) -> PathBuf {
    let common = match layout {
        Some(layout) => Some(layout.common.clone()),
        None if crate::diff::inside_git(workspace) => crate::diff::git_env(
            workspace,
            &[],
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .ok()
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from),
        None => None,
    };
    common
        .map(|common| {
            if common
                .file_name()
                .is_some_and(|name| name == crate::policy::git::GIT_DIR)
            {
                common.parent().map_or(common.clone(), Path::to_path_buf)
            } else {
                common
            }
        })
        .unwrap_or_else(|| {
            workspace
                .canonicalize()
                .unwrap_or_else(|_| workspace.to_path_buf())
        })
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
            | '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{FEFF}'
            | '\u{E0000}'..='\u{E007F}'
    )
}

#[cfg(test)]
#[path = "memory_tests.rs"]
mod tests;
