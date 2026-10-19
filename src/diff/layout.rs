use std::path::{Path, PathBuf};

use crate::policy::git::GIT_DIR;

const GIT_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_COMMON_DIR",
    "GIT_INDEX_FILE",
    "GIT_CEILING_DIRECTORIES",
    "GIT_DISCOVERY_ACROSS_FILESYSTEM",
];
pub(crate) const STATE_DIR: &str = "summonr";
const GITDIR: &str = "gitdir:";
const COMMONDIR: &str = "commondir";
const INDEX: &str = "index";
const WORKTREES: &str = "worktrees";
const GITDIR_FILE: &str = "gitdir";
const HEAD: &str = "HEAD";
const HEAD_REF: &str = "ref: refs/heads/";

pub(crate) struct Layout {
    pub(crate) git_dir: PathBuf,
    pub(crate) common: PathBuf,
    pub(crate) top: PathBuf,
}

impl Layout {
    pub(crate) fn index(&self) -> PathBuf {
        self.git_dir.join(INDEX)
    }

    pub(crate) fn checkouts(&self) -> Vec<PathBuf> {
        let primary = self
            .common
            .file_name()
            .filter(|name| *name == GIT_DIR)
            .and(self.common.parent())
            .map(Path::to_path_buf);
        let linked = std::fs::read_dir(self.common.join(WORKTREES))
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| owned(&entry.path()))
            .filter_map(|entry| std::fs::read_to_string(entry.path().join(GITDIR_FILE)).ok())
            .filter_map(|text| PathBuf::from(text.trim()).parent().map(Path::to_path_buf));
        primary.into_iter().chain(linked).collect()
    }
}

fn owned(admin: &Path) -> bool {
    std::fs::read_to_string(admin.join(HEAD))
        .ok()
        .as_deref()
        .and_then(|text| text.trim().strip_prefix(HEAD_REF))
        .and_then(crate::worktree::worktree_id)
        .is_some()
}

pub(crate) fn inside_git(root: &Path) -> bool {
    overridden() || root.ancestors().any(|path| path.join(GIT_DIR).exists())
}

pub(crate) fn layout(root: &Path) -> Option<Layout> {
    if overridden() {
        return None;
    }
    let root = dunce::canonicalize(root).ok()?;
    let top = root
        .ancestors()
        .find(|path| path.join(GIT_DIR).exists())?
        .to_path_buf();
    let dot = top.join(GIT_DIR);
    let git_dir = if dot.is_dir() {
        dot
    } else {
        let text = std::fs::read_to_string(&dot).ok()?;
        dunce::canonicalize(top.join(text.trim().strip_prefix(GITDIR)?.trim())).ok()?
    };
    let common = match std::fs::read_to_string(git_dir.join(COMMONDIR)) {
        Ok(text) => dunce::canonicalize(git_dir.join(text.trim())).ok()?,
        Err(_) => git_dir.clone(),
    };
    Some(Layout {
        git_dir,
        common,
        top,
    })
}

fn overridden() -> bool {
    GIT_ENV.iter().any(|name| std::env::var_os(name).is_some())
}

pub(crate) fn state_dir(root: &Path) -> Option<PathBuf> {
    if !inside_git(root) {
        return None;
    }
    let git_dir = match layout(root) {
        Some(layout) => layout.git_dir,
        None => PathBuf::from(
            super::git_env(root, &[], &["rev-parse", "--absolute-git-dir"])
                .ok()?
                .trim(),
        ),
    };
    Some(git_dir.join(STATE_DIR))
}
