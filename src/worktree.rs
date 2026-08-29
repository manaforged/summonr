use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tree {
    pub primary: PathBuf,
    pub path: PathBuf,
    pub branch: String,
    pub base: String,
}

#[must_use]
pub fn in_git(path: &Path) -> bool {
    primary(path).is_ok()
}

pub(crate) fn primary(path: &Path) -> Result<PathBuf> {
    let text = git(path, &["worktree", "list", "--porcelain"])?;
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
    let path = path.canonicalize().unwrap_or(path);
    let primary = primary.canonicalize().unwrap_or(primary);
    Ok(Tree {
        primary,
        path,
        branch,
        base,
    })
}

pub fn finish(tree: &Tree) -> Result<()> {
    if dirty(&tree.path)? {
        return Err(Error::Config(format!(
            "uncommitted files in {}",
            tree.path.display()
        )));
    }
    git(&tree.primary, &["merge", "--ff-only", "--", &tree.branch])?;
    remove(tree)
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
        .collect())
}

fn locate(primary: &Path, cwd: &Path) -> Result<Option<Tree>> {
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    Ok(list(primary)?
        .into_iter()
        .find(|tree| tree.path == cwd && tree.branch.starts_with("summon/")))
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
