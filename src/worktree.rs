use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result};

const COMMIT_MAX: u64 = 5 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tree {
    pub primary: PathBuf,
    pub path: PathBuf,
    pub branch: String,
    pub base: String,
}

/// One worktree with its distance from the base and the session that ran in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub tree: Tree,
    pub ahead: u32,
    pub dirty: bool,
    pub session: Option<String>,
}

pub fn status(cwd: &Path) -> Result<Vec<Status>> {
    let sessions = crate::session::list().unwrap_or_default();
    list(cwd)?
        .into_iter()
        .map(|tree| {
            let range = format!("{}..{}", tree.base, tree.branch);
            let ahead = git(&tree.primary, &["rev-list", "--count", &range])
                .ok()
                .and_then(|count| count.parse().ok())
                .unwrap_or(0);
            let dirty = dirty(&tree.path)?;
            let here = tree.path.display().to_string();
            let session = sessions
                .iter()
                .rev()
                .find(|info| info.cwd.as_deref() == Some(here.as_str()))
                .map(|info| info.id.clone());
            Ok(Status {
                tree,
                ahead,
                dirty,
                session,
            })
        })
        .collect()
}

/// Removes a worktree that holds nothing: no uncommitted change and no
/// commit the base lacks.
pub(crate) fn discard(tree: &Tree) -> Result<()> {
    if dirty(&tree.path)? {
        return Err(Error::Config(format!(
            "{} has uncommitted changes; merge or clean it first",
            tree.branch
        )));
    }
    let range = format!("{}..{}", tree.base, tree.branch);
    let ahead: u32 = git(&tree.primary, &["rev-list", "--count", &range])?
        .parse()
        .unwrap_or(0);
    if ahead > 0 {
        return Err(Error::Config(format!(
            "{} has {ahead} commit{} {} lacks; merge it first",
            tree.branch,
            if ahead == 1 { "" } else { "s" },
            tree.base
        )));
    }
    remove(tree)
}

#[must_use]
pub fn in_git(path: &Path) -> bool {
    primary(path).is_ok()
}

pub(crate) fn primary(path: &Path) -> Result<PathBuf> {
    let text = git(path, &["worktree", "list", "--porcelain"])
        .map_err(|_error| Error::Config(format!("{} is not a git repository", path.display())))?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("worktree ") {
            return Ok(PathBuf::from(rest));
        }
    }
    Err(Error::Config(format!(
        "{} is not a git repository",
        path.display()
    )))
}

pub fn add(cwd: &Path) -> Result<Tree> {
    let primary = primary(cwd)?;
    if let Some(existing) = locate(&primary, cwd)? {
        return Ok(existing);
    }
    let id = new_id();
    let branch = format!("summon/{id}");
    let path = worktree_dir(&primary, &id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let base = git(&primary, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if base == "HEAD" {
        return Err(Error::Config(format!(
            "{} is on a detached HEAD; check out a branch first, or run with --here",
            primary.display()
        )));
    }
    git(
        &primary,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            path.to_str().ok_or_else(|| {
                Error::Config(format!("worktree path is not utf-8: {}", path.display()))
            })?,
            "HEAD",
        ],
    )?;
    let record = base_record(&primary, &id)?;
    if let Some(parent) = record.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&record, format!("{base}\n"))?;
    let path = path.canonicalize().unwrap_or(path);
    let primary = primary.canonicalize().unwrap_or(primary);
    Ok(Tree {
        primary,
        path,
        branch,
        base,
    })
}

pub fn finish(tree: &Tree, objective: &str, paths: &[String]) -> Result<Vec<String>> {
    let mut committed = Vec::new();
    if dirty(&tree.path)? {
        let tracked = git(&tree.path, &["ls-files"])?;
        let tracked: std::collections::HashSet<&str> = tracked.lines().collect();
        let small: Vec<&str> = paths
            .iter()
            .map(String::as_str)
            .filter(|path| {
                std::fs::metadata(tree.path.join(path)).map_or_else(
                    |_missing| tracked.contains(path),
                    |meta| meta.len() <= COMMIT_MAX,
                )
            })
            .collect();
        if !small.is_empty() {
            let mut args = vec!["add", "--all", "--"];
            args.extend(small.iter().copied());
            git(&tree.path, &args)?;
            git(&tree.path, &["commit", "-q", "-m", &subject(objective)])?;
            committed = small.iter().map(|path| (*path).to_string()).collect();
        }
        let leftover = git(&tree.path, &["status", "--porcelain"])?;
        if !leftover.is_empty() {
            return Err(Error::Config(format!(
                "left uncommitted in {} (not part of this run, or over {} bytes):\n{leftover}",
                tree.path.display(),
                COMMIT_MAX
            )));
        }
    }
    let big: Vec<String> = git(
        &tree.primary,
        &[
            "diff",
            "--name-only",
            &format!("{}..{}", tree.base, tree.branch),
        ],
    )?
    .lines()
    .filter(|path| {
        std::fs::metadata(tree.path.join(path)).is_ok_and(|meta| meta.len() > COMMIT_MAX)
    })
    .map(str::to_string)
    .collect();
    if !big.is_empty() {
        return Err(Error::Config(format!(
            "files over {COMMIT_MAX} bytes on {}: {} — remove them from the branch before it can merge",
            tree.branch,
            big.join(", ")
        )));
    }
    let head = git(&tree.primary, &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if head != tree.base {
        return Err(Error::Config(format!(
            "{} is on {head}; the run started on {}",
            tree.primary.display(),
            tree.base
        )));
    }
    git(&tree.primary, &["merge", "--ff-only", "--", &tree.branch])?;
    remove(tree)?;
    Ok(committed)
}

fn subject(objective: &str) -> String {
    let text: String = objective
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(42)
        .collect();
    let mut chars = text.trim_end_matches(['.', ' ']).chars();
    let head: String = chars
        .next()
        .map(|c| c.to_lowercase().collect())
        .unwrap_or_default();
    format!("summon: {head}{}", chars.as_str())
}

pub(crate) fn remove(tree: &Tree) -> Result<()> {
    let path = tree.path.to_str().ok_or_else(|| {
        Error::Config(format!(
            "worktree path is not utf-8: {}",
            tree.path.display()
        ))
    })?;
    git(&tree.primary, &["worktree", "remove", "--force", path])?;
    drop(git(&tree.primary, &["branch", "-d", &tree.branch]));
    if let Some(id) = tree.branch.strip_prefix("summon/")
        && let Ok(record) = base_record(&tree.primary, id)
    {
        drop(std::fs::remove_file(record));
    }
    Ok(())
}

pub fn sweep(cwd: &Path) -> Result<Vec<PathBuf>> {
    let Ok(primary) = primary(cwd) else {
        return Ok(Vec::new());
    };
    drop(git(&primary, &["worktree", "prune"]));
    Ok(Vec::new())
}

pub fn list(cwd: &Path) -> Result<Vec<Tree>> {
    let primary = primary(cwd)?;
    let base = git(&primary, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let text = git(&primary, &["worktree", "list", "--porcelain"])?;
    Ok(parse_list(&primary, &base, &text)
        .into_iter()
        .filter(|tree| tree.branch.starts_with("summon/"))
        .map(|mut tree| {
            if let Some(id) = tree.branch.strip_prefix("summon/")
                && let Ok(record) = base_record(&primary, id)
                && let Ok(recorded) = std::fs::read_to_string(record)
                && !recorded.trim().is_empty()
            {
                tree.base = recorded.trim().to_string();
            }
            tree
        })
        .collect())
}

fn locate(primary: &Path, cwd: &Path) -> Result<Option<Tree>> {
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    Ok(list(primary)?
        .into_iter()
        .find(|tree| tree.path == cwd && tree.branch.starts_with("summon/")))
}

fn base_record(primary: &Path, id: &str) -> Result<PathBuf> {
    let common = git(primary, &["rev-parse", "--git-common-dir"])?;
    let common = Path::new(&common);
    let common = if common.is_absolute() {
        common.to_path_buf()
    } else {
        primary.join(common)
    };
    Ok(common.join("summon").join(format!("{id}.base")))
}

fn worktree_dir(primary: &Path, id: &str) -> Result<PathBuf> {
    let name = primary
        .file_name()
        .ok_or_else(|| Error::Config("repository path has no name".to_string()))?;
    let parent = primary
        .parent()
        .ok_or_else(|| Error::Config("repository path has no parent".to_string()))?;
    let mut dir = parent.to_path_buf();
    dir.push(format!("{}-worktrees", name.to_string_lossy()));
    dir.push(id);
    Ok(dir)
}

fn dirty(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    Ok(!git(path, &["status", "--porcelain"])?.is_empty())
}

fn parse_list(primary: &Path, base: &str, text: &str) -> Vec<Tree> {
    let mut trees = Vec::new();
    let mut path = None;
    let mut branch = None;
    for line in text.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if let (Some(path), Some(branch)) = (path.take(), branch.take()) {
                trees.push(Tree {
                    primary: primary.to_path_buf(),
                    path,
                    branch,
                    base: base.to_string(),
                });
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("worktree ") {
            path = Some(PathBuf::from(rest));
        } else if let Some(rest) = line.strip_prefix("branch refs/heads/") {
            branch = Some(rest.to_string());
        } else if line == "detached" {
            branch = Some(String::new());
        }
    }
    trees
}

fn new_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{nanos:x}")
}

fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(["-C"])
        .arg(repo)
        .args(args)
        .output()?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let err = err.trim();
        return Err(Error::Config(if err.is_empty() {
            format!("git {} failed", args.first().unwrap_or(&"command"))
        } else {
            err.to_string()
        }));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
#[path = "worktree_tests.rs"]
mod tests;
