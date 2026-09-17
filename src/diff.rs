use std::path::Path;
use std::process::Command;

use serde::Serialize;

use crate::durable::log::{self, Line};
use crate::error::{Error, Result};
use crate::worktree;

const EDITORS: [&str; 9] = [
    "edit",
    "edit_file",
    "write",
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

fn base(root: &Path) -> Result<Option<String>> {
    let base = worktree::list(root)?
        .into_iter()
        .find(|tree| tree.path == root)
        .map_or_else(|| "HEAD".to_string(), |tree| tree.base);
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
    if !root.ancestors().any(|path| path.join(".git").exists()) {
        return "non-Git workspace".to_string();
    }
    let Ok(base) = base(root) else {
        return "Git state unavailable".to_string();
    };
    let head = match git(root, &["rev-parse", "--short=12", "HEAD"]) {
        Ok(head) => head.trim().to_string(),
        Err(_) if base.is_none() || initial(root).unwrap_or(false) => "no commits".to_string(),
        Err(_) => return "Git state unavailable".to_string(),
    };
    let base = base.as_deref().unwrap_or("empty tree");
    match stats(root) {
        Ok(diff) if diff.files.is_empty() => {
            format!("{head} ({}), worktree clean", short(base))
        }
        Ok(diff) => format!(
            "{} ({}), {} file{} dirty",
            head,
            short(base),
            diff.files.len(),
            if diff.files.len() == 1 { "" } else { "s" }
        ),
        Err(error) => format!(
            "{} ({}), worktree state unavailable: {error}",
            head,
            short(base)
        ),
    }
}

fn short(value: &str) -> &str {
    value.get(..12).unwrap_or(value)
}

pub fn stats(root: &Path) -> Result<Diff> {
    let base = base(root)?;
    let reference = reference(root, base.as_deref())?;
    let numstat = git(
        root,
        &["diff", "--numstat", "--no-renames", &reference, "--"],
    )?;
    let mut files: Vec<Stat> = numstat.lines().filter_map(numstat_row).collect();
    let untracked = git(root, &["ls-files", "-z", "--others", "--exclude-standard"])?;
    for path in untracked.split('\0').filter(|path| !path.is_empty()) {
        let mut stat = if path.ends_with('/') {
            Stat::default()
        } else {
            let text = git(
                root,
                &["diff", "--no-index", "--numstat", "--", "/dev/null", path],
            )?;
            text.lines()
                .find_map(numstat_row)
                .ok_or_else(|| Error::Config(format!("git returned no line counts for {path}")))?
        };
        stat.path = path.to_string();
        stat.new = true;
        files.push(stat);
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Diff {
        base: base.unwrap_or_else(|| "empty tree".to_string()),
        files,
    })
}

pub fn patch(root: &Path, path: Option<&str>) -> Result<String> {
    let base = reference(root, base(root)?.as_deref())?;
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
        if file.ends_with('/') {
            if path.is_some() {
                return Err(Error::Config(format!(
                    "open {file} as a separate workspace to inspect its changes"
                )));
            }
            continue;
        }
        text.push_str(&git(
            root,
            &["diff", "--no-index", "--no-color", "--", "/dev/null", file],
        )?);
    }
    Ok(text)
}

pub fn hunks(root: &Path, path: &str) -> Result<Vec<(usize, usize)>> {
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
    git_env(root, &[], args)
}

pub(crate) fn git_env(
    root: &Path,
    env: &[(&str, &std::ffi::OsStr)],
    args: &[&str],
) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .envs(env.iter().copied())
        .args(args)
        .output()?;
    let different = args.starts_with(&["diff", "--no-index"])
        && output.status.code() == Some(1)
        && !output.stdout.is_empty();
    if !output.status.success() && !different {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(Error::Config(format!(
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
