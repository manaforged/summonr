use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::durable::store;
use crate::error::{Error, Result};

mod land;

pub use land::finish;
pub(crate) use land::{reconcile_children, remove_child};

const COMMIT_MAX: u64 = 5 * 1024 * 1024;

fn worktree_id(branch: &str) -> Option<&str> {
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
}

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
            let ahead = crate::diff::git_env(&tree.primary, &[], &["rev-list", "--count", &range])
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

pub(crate) fn discard(tree: &Tree) -> Result<()> {
    if dirty(&tree.path)? {
        return Err(Error::Config(format!(
            "{} has uncommitted changes; merge or clean it first",
            tree.branch
        )));
    }
    let range = format!("{}..{}", tree.base, tree.branch);
    let ahead: u32 = crate::diff::git_env(&tree.primary, &[], &["rev-list", "--count", &range])?
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
    crate::diff::git_env(
        primary,
        &[],
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            path.to_str().ok_or_else(|| {
                Error::Config(format!("worktree path is not utf-8: {}", path.display()))
            })?,
            start,
        ],
    )?;
    let path = path.canonicalize().unwrap_or(path);
    let primary = primary
        .canonicalize()
        .unwrap_or_else(|_| primary.to_path_buf());
    let tree = Tree {
        primary,
        path,
        branch,
        base: base.to_string(),
    };
    let recorded = (|| {
        let record = base_record(&tree.primary, &id)?;
        store::write(&record, format!("{base}\n"))?;
        Ok(())
    })();
    if let Err(error) = recorded {
        drop(remove_child(&tree));
        return Err(error);
    }
    Ok(tree)
}

pub(crate) fn same_repository(left: &Path, right: &Path) -> bool {
    match (primary(left), primary(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

pub(crate) fn remove(tree: &Tree) -> Result<()> {
    let path = tree.path.to_str().ok_or_else(|| {
        Error::Config(format!(
            "worktree path is not utf-8: {}",
            tree.path.display()
        ))
    })?;
    crate::diff::git_env(&tree.primary, &[], &["worktree", "remove", "--force", path])?;
    drop(crate::diff::git_env(
        &tree.primary,
        &[],
        &["branch", "-d", &tree.branch],
    ));
    if let Some(id) = worktree_id(&tree.branch)
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
    drop(crate::diff::git_env(&primary, &[], &["worktree", "prune"]));
    Ok(Vec::new())
}

pub fn list(cwd: &Path) -> Result<Vec<Tree>> {
    let (primary, text) = listing(cwd)?;
    let base = crate::diff::git_env(&primary, &[], &["rev-parse", "--abbrev-ref", "HEAD"])
        .unwrap_or_default();
    Ok(parse_list(&primary, &base, &text)
        .into_iter()
        .filter(|tree| worktree_id(&tree.branch).is_some())
        .map(|mut tree| {
            if let Some(id) = worktree_id(&tree.branch)
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
        .find(|tree| tree.path == cwd && worktree_id(&tree.branch).is_some()))
}

fn base_record(primary: &Path, id: &str) -> Result<PathBuf> {
    let common = crate::diff::git_env(primary, &[], &["rev-parse", "--git-common-dir"])?;
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

pub(crate) fn dirty(path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    Ok(!crate::diff::git_env(path, &[], &["status", "--porcelain"])?.is_empty())
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
