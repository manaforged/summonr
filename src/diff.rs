use std::path::Path;

use serde::Serialize;

use crate::durable::log::{self, Line};
use crate::error::{Error, Result};
use crate::worktree;

mod git;
pub(crate) mod ignored;
mod layout;
mod turn;

use git::{git, numstat_row};
pub(crate) use git::{git_env, git_output, git_paths};
pub(crate) use layout::{Layout, STATE_DIR, inside_git, layout, state_dir};
pub use turn::{SESSION_BASE, TurnId, turn_patch, turn_stats};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Stat {
    pub path: String,
    pub added: u32,
    pub removed: u32,
    pub new: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Diff {
    pub base: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    pub files: Vec<Stat>,
}

impl Diff {
    #[must_use]
    pub fn added(&self) -> u32 {
        self.files.iter().map(|stat| stat.added).sum()
    }

    #[must_use]
    pub fn removed(&self) -> u32 {
        self.files.iter().map(|stat| stat.removed).sum()
    }

    #[must_use]
    pub fn summary(&self) -> String {
        let scope = match &self.session {
            Some(session) => format!(
                "{} of session {}",
                self.base,
                crate::session::short(session)
            ),
            None => format!("vs {}", self.base),
        };
        format!(
            "{} · +{} −{} · {scope}",
            crate::text::plural(self.files.len(), "file"),
            self.added(),
            self.removed(),
        )
    }
}

fn named(root: &Path) -> Result<String> {
    Ok(worktree::tree_at(root).map_or_else(|| "HEAD".to_string(), |tree| tree.base))
}

fn base(root: &Path) -> Result<Option<String>> {
    resolve(root, named(root)?)
}

fn resolve(root: &Path, base: String) -> Result<Option<String>> {
    match git(root, &["rev-parse", "--verify", &base]) {
        Ok(_) => Ok(Some(base)),
        Err(error) => {
            if base == "HEAD" && initial(root)? {
                Ok(None)
            } else {
                Err(error)
            }
        }
    }
}

fn initial(root: &Path) -> Result<bool> {
    Ok(git(
        root,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "--untracked-files=no",
        ],
    )?
    .lines()
    .any(|line| line == "# branch.oid (initial)"))
}

fn reference(root: &Path, base: Option<&str>) -> Result<String> {
    match base {
        Some(base) => Ok(base.to_string()),
        None => git(root, &["hash-object", "-t", "tree", "--stdin"])
            .map(|value| value.trim().to_string()),
    }
}

pub(crate) fn snapshot(root: &Path) -> String {
    if !inside_git(root) {
        return "non-Git workspace".to_string();
    }
    let Ok((head, base, count)) = dirty(root) else {
        return "Git state unavailable".to_string();
    };
    match count {
        0 => format!("{head} ({}), worktree clean", abbrev(&base)),
        count => format!(
            "{head} ({}), {} dirty",
            abbrev(&base),
            crate::text::plural(count, "file")
        ),
    }
}

fn dirty(root: &Path) -> Result<(String, String, usize)> {
    let named = named(root)?;
    if named != "HEAD" {
        let base = resolve(root, named)?;
        let diff = changes(root, base, false)?;
        let head = git(root, &["rev-parse", "--short=12", "HEAD"])
            .unwrap_or_else(|_| "no commits".to_string());
        return Ok((head, diff.base, diff.files.len()));
    }
    let status = git(
        root,
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--no-renames",
            "--untracked-files=all",
        ],
    )?;
    let mut head = None;
    let mut count = 0;
    for record in status.split('\0').filter(|record| !record.is_empty()) {
        match record.strip_prefix("# branch.oid ") {
            Some(oid) => head = Some(oid.to_string()),
            None if record.starts_with('#') => {}
            None if status_path(record).is_some_and(ignored::hidden) => {}
            None => count += 1,
        }
    }
    Ok(match head.filter(|oid| oid != "(initial)") {
        Some(oid) => (abbrev(&oid).to_string(), named, count),
        None => ("no commits".to_string(), "empty tree".to_string(), count),
    })
}

fn status_path(record: &str) -> Option<&str> {
    let fields = match record.as_bytes().first() {
        Some(b'1') => 9,
        Some(b'u') => 11,
        Some(b'?' | b'!') => 2,
        Some(_) | None => return None,
    };
    record.splitn(fields, ' ').nth(fields - 1)
}

fn abbrev(oid: &str) -> &str {
    oid.get(..12).unwrap_or(oid)
}

pub fn stats(root: &Path) -> Result<Diff> {
    changes(root, base(root)?, true)
}

fn changes(root: &Path, base: Option<String>, lines: bool) -> Result<Diff> {
    let reference = reference(root, base.as_deref())?;
    let numstat = git(
        root,
        &["diff", "--numstat", "--no-renames", &reference, "--"],
    )?;
    let mut files: Vec<Stat> = numstat
        .lines()
        .filter_map(numstat_row)
        .filter(|stat| !ignored::hidden(&stat.path))
        .collect();
    for path in untracked(root)? {
        files.push(Stat {
            added: if lines { added(root, &path)? } else { 0 },
            path,
            removed: 0,
            new: true,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Diff {
        base: base.unwrap_or_else(|| "empty tree".to_string()),
        session: None,
        files,
    })
}

fn added(root: &Path, path: &str) -> Result<u32> {
    if path.ends_with('/') {
        return Ok(0);
    }
    let lines = line_count(&root.join(path))?.unwrap_or(0);
    Ok(u32::try_from(lines).unwrap_or(u32::MAX))
}

pub(crate) fn line_count(path: &Path) -> Result<Option<u64>> {
    use std::io::BufRead;
    let mut reader = std::io::BufReader::new(std::fs::File::open(path)?);
    let mut lines = 0_u64;
    let mut head = true;
    let mut last = b'\n';
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            return Ok(Some(lines.saturating_add(u64::from(last != b'\n'))));
        }
        if std::mem::take(&mut head) && chunk[..chunk.len().min(8000)].contains(&0) {
            return Ok(None);
        }
        lines += chunk.iter().filter(|byte| **byte == b'\n').count() as u64;
        last = *chunk.last().unwrap_or(&b'\n');
        let size = chunk.len();
        reader.consume(size);
    }
}

pub fn patch(root: &Path, path: Option<&str>) -> Result<String> {
    let base = reference(root, base(root)?.as_deref())?;
    let mut text = tracked_patch(root, &base, path)?;
    text.push_str(&untracked_patch(root, path)?);
    Ok(text)
}

fn tracked_patch(root: &Path, base: &str, path: Option<&str>) -> Result<String> {
    let shown: Vec<String> = match path {
        Some(path) => vec![path.to_string()],
        None => git_paths(
            root,
            &["diff", "--name-only", "-z", "--no-renames", base, "--"],
        )?
        .iter()
        .filter(|file| !ignored::hidden(file))
        .map(|file| format!(":(literal){file}"))
        .collect(),
    };
    if shown.is_empty() {
        return Ok(String::new());
    }
    let mut args = vec!["diff", "--no-color", "--no-renames", base, "--"];
    args.extend(shown.iter().map(String::as_str));
    Ok(String::from_utf8_lossy(&git_output(root, &[], &args, None)?).into_owned())
}

fn untracked_patch(root: &Path, path: Option<&str>) -> Result<String> {
    let mut text = String::new();
    for file in untracked(root)?
        .iter()
        .map(String::as_str)
        .filter(|file| path.is_none_or(|path| path == *file))
    {
        if file.ends_with('/') {
            if path.is_some() {
                return Err(Error::Config(format!(
                    "open {file} as a separate workspace to inspect its changes"
                )));
            }
            continue;
        }
        let args = ["diff", "--no-index", "--no-color", "--", "/dev/null", file];
        text.push_str(&String::from_utf8_lossy(&git_output(
            root,
            &[],
            &args,
            None,
        )?));
    }
    Ok(text)
}

fn untracked(root: &Path) -> Result<Vec<String>> {
    let mut paths = git_paths(root, &["ls-files", "-z", "--others", "--exclude-standard"])?;
    paths.retain(|path| !ignored::hidden(path));
    Ok(paths)
}

pub(crate) fn hunks(root: &Path, path: &str) -> Result<Vec<(usize, usize)>> {
    let tracked = !git(root, &["ls-files", "-z", "--", path])?.is_empty();
    let text = if tracked {
        let base = reference(root, base(root)?.as_deref())?;
        git(root, &["diff", "-U0", "--no-color", &base, "--", path])?
    } else {
        git(
            root,
            &[
                "diff",
                "--no-index",
                "-U0",
                "--no-color",
                "--",
                "/dev/null",
                path,
            ],
        )?
    };
    Ok(text.lines().filter_map(hunk_header).collect())
}

fn hunk_header(line: &str) -> Option<(usize, usize)> {
    let rest = line.strip_prefix("@@ ")?;
    let plus = rest.split(' ').find(|word| word.starts_with('+'))?;
    let (start, count) = plus[1..].split_once(',').unwrap_or((&plus[1..], "1"));
    let count: usize = count.parse().ok()?;
    (count > 0).then(|| (start.parse().ok().unwrap_or(1), count))
}

pub(crate) fn edited<'a>(calls: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for (name, arguments) in calls {
        if !crate::tools::Builtin::of(name).is_some_and(crate::tools::Builtin::edits) {
            continue;
        }
        let Ok(args) = serde_json::from_str::<serde_json::Value>(arguments) else {
            continue;
        };
        let Some(path) = args.get("path").and_then(serde_json::Value::as_str) else {
            continue;
        };
        if !paths.iter().any(|seen| seen == path) {
            paths.push(path.to_string());
        }
    }
    paths
}

pub fn turns(journal: &Path) -> Result<Vec<Vec<String>>> {
    let mut turns: Vec<Vec<String>> = Vec::new();
    for line in log::read(journal)? {
        match line {
            Line::User { start: true, .. } => turns.push(Vec::new()),
            Line::Assistant { tool_calls, .. } => {
                let Some(turn) = turns.last_mut() else {
                    continue;
                };
                for path in edited(tool_calls.iter().map(|call| {
                    (
                        call.function.name.as_str(),
                        call.function.arguments.as_str(),
                    )
                })) {
                    if !turn.contains(&path) {
                        turn.push(path);
                    }
                }
            }
            _ => {}
        }
    }
    Ok(turns)
}

#[cfg(test)]
#[path = "diff_tests.rs"]
mod tests;
