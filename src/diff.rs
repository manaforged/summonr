use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::durable::log::{self, Line};
use crate::error::{Error, Result};
use crate::worktree;

const EDITORS: [&str; 7] = [
    "edit_file",
    "write_file",
    "replace_text",
    "replace_texts",
    "replace_lines",
    "insert_after",
    "delete_lines",
];

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
        let count = self.files.len();
        format!(
            "{count} file{} · +{} −{} · vs {}",
            if count == 1 { "" } else { "s" },
            self.added(),
            self.removed(),
            self.base
        )
    }
}

/// The revision the workspace is compared against: the worktree base when
/// `root` is a Summoner worktree, `HEAD` otherwise.
#[must_use]
pub fn base(root: &Path) -> String {
    worktree::list(root)
        .ok()
        .and_then(|trees| trees.into_iter().find(|tree| tree.path == root))
        .map_or_else(|| "HEAD".to_string(), |tree| tree.base)
}

pub(crate) fn snapshot(root: &Path) -> String {
    if !root.ancestors().any(|path| path.join(".git").exists()) {
        return "non-Git workspace".to_string();
    }
    let Ok(head) = git(root, &["rev-parse", "--short=12", "HEAD"]) else {
        return "Git state unavailable".to_string();
    };
    let base = base(root);
    match stats(root) {
        Ok(diff) if diff.files.is_empty() => {
            format!("{} ({}), worktree clean", head.trim(), short(&base))
        }
        Ok(diff) => format!(
            "{} ({}), {} file{} dirty",
            head.trim(),
            short(&base),
            diff.files.len(),
            if diff.files.len() == 1 { "" } else { "s" }
        ),
        Err(error) => format!(
            "{} ({}), worktree state unavailable: {error}",
            head.trim(),
            short(&base)
        ),
    }
}

fn short(value: &str) -> &str {
    value.get(..12).unwrap_or(value)
}

/// Per-file line counts for every change in `root` against [`base`],
/// untracked files included.
pub fn stats(root: &Path) -> Result<Diff> {
    let base = base(root);
    let numstat = git(root, &["diff", "--numstat", "--no-renames", &base, "--"])?;
    let mut files: Vec<Stat> = numstat.lines().filter_map(numstat_row).collect();
    let untracked = git(root, &["ls-files", "-z", "--others", "--exclude-standard"])?;
    for path in untracked.split('\0').filter(|path| !path.is_empty()) {
        let added = std::fs::read(root.join(path))
            .map(|bytes| bytes.iter().filter(|byte| **byte == b'\n').count() as u32)
            .unwrap_or(0);
        files.push(Stat {
            path: path.to_string(),
            added,
            removed: 0,
            new: true,
        });
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Diff { base, files })
}

/// The unified patch for `path`, or for the whole workspace when `path` is
/// `None`, against [`base`].
pub fn patch(root: &Path, path: Option<&str>) -> Result<String> {
    let base = base(root);
    let mut args = vec!["diff", "--no-color", "--no-renames", base.as_str(), "--"];
    if let Some(path) = path {
        args.push(path);
    }
    let mut text = git(root, &args)?;
    let untracked = git(root, &["ls-files", "-z", "--others", "--exclude-standard"])?;
    for file in untracked
        .split('\0')
        .filter(|file| !file.is_empty() && path.is_none_or(|path| path == *file))
    {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["diff", "--no-color", "--no-index", "--", "/dev/null", file])
            .output()?;
        text.push_str(&String::from_utf8_lossy(&output.stdout));
    }
    Ok(text)
}

/// Added line ranges of `path` against [`base`] as `(first line, count)`,
/// 1-based. An untracked file is one range covering every line.
pub fn hunks(root: &Path, path: &str) -> Result<Vec<(usize, usize)>> {
    let tracked = !git(root, &["ls-files", "--", path])?.trim().is_empty();
    if !tracked {
        let lines = std::fs::read_to_string(root.join(path))
            .map(|text| text.lines().count())
            .unwrap_or(0);
        return Ok(if lines == 0 {
            Vec::new()
        } else {
            vec![(1, lines)]
        });
    }
    let base = base(root);
    let text = git(root, &["diff", "-U0", "--no-color", &base, "--", path])?;
    Ok(text.lines().filter_map(hunk_header).collect())
}

fn hunk_header(line: &str) -> Option<(usize, usize)> {
    let rest = line.strip_prefix("@@ ")?;
    let plus = rest.split(' ').find(|word| word.starts_with('+'))?;
    let (start, count) = plus[1..].split_once(',').unwrap_or((&plus[1..], "1"));
    let count: usize = count.parse().ok()?;
    (count > 0).then(|| (start.parse().ok().unwrap_or(1), count))
}

/// Workspace paths the given tool calls edit, in call order, deduplicated.
pub fn edited<'a>(calls: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for (name, arguments) in calls {
        if !EDITORS.contains(&name) {
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

/// The paths each turn of a session journal edited, first turn first.
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

fn numstat_row(line: &str) -> Option<Stat> {
    let mut parts = line.splitn(3, '\t');
    let added = parts.next()?.trim();
    let removed = parts.next()?.trim();
    let path = parts.next()?.trim();
    if path.is_empty() {
        return None;
    }
    Some(Stat {
        path: path.to_string(),
        added: added.parse().unwrap_or(0),
        removed: removed.parse().unwrap_or(0),
        new: false,
    })
}

fn git(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Config(format!(
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
#[path = "diff_tests.rs"]
mod tests;
