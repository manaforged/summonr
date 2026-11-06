use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use summonr_core::error::{Error, Result};

mod base;
mod delta;
mod finish;
mod land;
mod remove;

pub use finish::finish;
pub(crate) use land::{Landing, reconcile_children};
pub(crate) use remove::discard;

const COMMIT_MAX: u64 = 5 * 1024 * 1024;
const UNKNOWN: &str = "?";

pub(crate) fn worktree_id(branch: &str) -> Option<&str> {
    branch
        .strip_prefix("summonr/")
        .or_else(|| branch.strip_prefix("summon/"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tree {
    pub primary: PathBuf,
    pub path: PathBuf,
    pub branch: String,
    pub base: String,
    pub snapshot: Option<String>,
}

/// Commits a worktree branch has past its recorded base; `None` when no base is recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct Ahead(pub Option<u32>);

impl std::fmt::Display for Ahead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Some(count) => write!(f, "{count}"),
            None => f.write_str(UNKNOWN),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub tree: Tree,
    pub ahead: Ahead,
    pub dirty: bool,
    pub session: Option<String>,
}

pub fn status(cwd: &Path) -> Result<Vec<Status>> {
    let sessions = crate::session::list().unwrap_or_default();
    list(cwd)?
        .into_iter()
        .map(|tree| {
            let range = format!("{}..{}", tree.base, tree.branch);
            let ahead = Ahead(if tree.base.is_empty() {
                None
            } else {
                crate::diff::git_env(&tree.primary, &[], &["rev-list", "--count", &range])
                    .ok()
                    .and_then(|count| count.parse().ok())
            });
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

#[must_use]
pub fn in_git(path: &Path) -> bool {
    primary(path).is_ok()
}

pub(crate) fn primary(path: &Path) -> Result<PathBuf> {
    listing(path).map(|(primary, _)| primary)
}

fn listing(path: &Path) -> Result<(PathBuf, String)> {
    let missing = || Error::Config(format!("{} is not a git repository", path.display()));
    let text = crate::diff::git_env(path, &[], &["worktree", "list", "--porcelain"])
        .map_err(|_error| missing())?;
    let primary = text
        .lines()
        .find_map(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
        .ok_or_else(missing)?;
    Ok((primary, text))
}

pub fn add(cwd: &Path) -> Result<Tree> {
    let primary = primary(cwd)?;
    if let Some(existing) = locate(&primary, cwd)? {
        return Ok(existing);
    }
    let base = crate::diff::git_env(&primary, &[], &["rev-parse", "--abbrev-ref", "HEAD"])?;
    if base == "HEAD" {
        return Err(Error::Config(format!(
            "{} is on a detached HEAD; check out a branch first, or run with --here",
            primary.display()
        )));
    }
    add_from(&primary, &base, "HEAD")
}

fn add_from(primary: &Path, base: &str, start: &str) -> Result<Tree> {
    let id = new_id();
    let branch = format!("summonr/{id}");
    let path = worktree_dir(primary, &id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let target = path
        .to_str()
        .ok_or_else(|| Error::Config(format!("worktree path is not utf-8: {}", path.display())))?;
    let record = base::save(primary, &id, base)?;
    let added = crate::diff::git_env(
        primary,
        &[],
        &["worktree", "add", "-b", &branch, target, start],
    );
    if let Err(error) = added {
        drop(std::fs::remove_file(&record));
        return Err(error);
    }
    let path = dunce::canonicalize(&path).unwrap_or(path);
    let primary = dunce::canonicalize(primary).unwrap_or_else(|_| primary.to_path_buf());
    Ok(Tree {
        primary,
        path,
        branch,
        base: base.to_string(),
        snapshot: None,
    })
}

pub(crate) fn is_delta_of(workspace: &Path, candidate: &Path) -> bool {
    let Ok(primary) = primary(workspace) else {
        return false;
    };
    let primary = primary.canonicalize().unwrap_or_else(|_| primary.clone());
    let (Some(name), Some(parent)) = (primary.file_name(), primary.parent()) else {
        return false;
    };
    let root = parent.join(format!("{}-{CHILDREN}", name.to_string_lossy()));
    let root = root.canonicalize().unwrap_or(root);
    let Some(actual) = candidate.parent() else {
        return false;
    };
    let actual = actual
        .canonicalize()
        .unwrap_or_else(|_| actual.to_path_buf());
    actual == root && candidate.file_name().is_some()
}

impl Tree {
    pub fn unchanged(&self) -> Result<bool> {
        Ok(self.changes()?.is_empty())
    }

    pub fn changes(&self) -> Result<Vec<String>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let base = self.snapshot.as_deref().unwrap_or("HEAD");
        let diff = crate::diff::git_paths(&self.path, &["diff", "--name-only", "-z", base])?;
        let untracked = crate::diff::git_paths(
            &self.path,
            &["ls-files", "-z", "--others", "--exclude-standard"],
        )?;
        Ok(diff
            .into_iter()
            .chain(untracked)
            .filter(|path| !crate::diff::ignored::hidden(path))
            .collect())
    }
}

pub fn sweep(cwd: &Path) -> Result<Vec<PathBuf>> {
    let Ok((primary, before)) = listing(cwd) else {
        return Ok(Vec::new());
    };
    crate::diff::git_env(&primary, &[], &["worktree", "prune"])?;
    let (_, after) = listing(&primary)?;
    let kept: std::collections::HashSet<&str> = after
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .collect();
    Ok(before
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .filter(|path| !kept.contains(path))
        .map(PathBuf::from)
        .collect())
}

pub fn list(cwd: &Path) -> Result<Vec<Tree>> {
    let (primary, text) = listing(cwd)?;
    Ok(parse_list(&primary, &text)
        .into_iter()
        .filter(|tree| worktree_id(&tree.branch).is_some())
        .map(|mut tree| {
            tree.base = base::recorded(&primary, &tree.branch).unwrap_or_default();
            tree
        })
        .collect())
}

pub fn tree_at(cwd: &Path) -> Option<Tree> {
    if !crate::diff::inside_git(cwd) {
        return None;
    }
    if crate::diff::layout(cwd).is_some_and(|layout| layout.git_dir == layout.common) {
        return None;
    }
    locate(cwd, cwd).ok().flatten()
}

fn locate(primary: &Path, cwd: &Path) -> Result<Option<Tree>> {
    let cwd = crate::durable::driver::canonical(cwd);
    Ok(list(primary)?.into_iter().find(|tree| {
        crate::durable::driver::canonical(&tree.path) == cwd && worktree_id(&tree.branch).is_some()
    }))
}

pub(crate) fn common_dir(primary: &Path) -> Result<PathBuf> {
    let common = crate::diff::git_env(primary, &[], &["rev-parse", "--git-common-dir"])?;
    Ok(primary.join(common))
}

#[cfg(feature = "gui")]
pub(crate) fn display_name(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    Some(repo_of(path).map_or(name, |repo| format!("{repo} · worktree")))
}

#[cfg(feature = "gui")]
/// The repository a summonr worktree at `path` belongs to; `None` for any other folder.
pub(crate) fn repo_of(path: &Path) -> Option<String> {
    path.parent()
        .and_then(Path::file_name)
        .map(|parent| parent.to_string_lossy().into_owned())
        .and_then(|parent| {
            parent
                .strip_suffix(&format!("-{WORKTREES}"))
                .map(str::to_string)
        })
}

const WORKTREES: &str = "worktrees";

fn worktree_dir(primary: &Path, id: &str) -> Result<PathBuf> {
    checkout_dir(primary, WORKTREES, id)
}

pub(super) const CHILDREN: &str = "children";

fn checkout_dir(primary: &Path, kind: &str, id: &str) -> Result<PathBuf> {
    let name = primary
        .file_name()
        .ok_or_else(|| Error::Config("repository path has no name".to_string()))?;
    let parent = primary
        .parent()
        .ok_or_else(|| Error::Config("repository path has no parent".to_string()))?;
    let mut dir = parent.to_path_buf();
    dir.push(format!("{}-{kind}", name.to_string_lossy()));
    dir.push(id);
    Ok(dir)
}

pub(crate) fn open_delta(primary: &Path) -> Result<Tree> {
    delta::open(primary)
}

pub(crate) fn dirty(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    Ok(!crate::diff::git_env(path, &[], &["status", "--porcelain"])?.is_empty())
}

fn parse_list(primary: &Path, text: &str) -> Vec<Tree> {
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
                    base: String::new(),
                    snapshot: None,
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

#[cfg(test)]
#[path = "worktree_tests.rs"]
pub(crate) mod tests;
