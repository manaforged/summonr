use std::path::{Path, PathBuf};

use summonr_agent::worktree::Tree;

use super::{App, read_git_branch};

/// The folder the active chat works in, with its path spellings, git branch and its repository's worktrees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Place {
    pub(crate) workspace: PathBuf,
    pub(crate) roots: Vec<String>,
    pub(crate) git_branch: Option<String>,
    pub(crate) worktrees: Vec<Tree>,
}

impl Place {
    pub(crate) fn at(workspace: &Path) -> Self {
        let git_branch = read_git_branch(workspace);
        let worktrees = git_branch
            .as_ref()
            .and_then(|_| summonr_agent::worktree::linked(workspace).ok())
            .unwrap_or_default();
        Self {
            workspace: workspace.to_path_buf(),
            roots: roots(workspace),
            git_branch,
            worktrees,
        }
    }
}

impl App {
    /// The folder the active chat works in.
    pub(crate) fn workspace(&self) -> PathBuf {
        self.place.with(|place| place.workspace.clone())
    }

    /// The spellings of the active folder used to shorten paths.
    pub(crate) fn roots(&self) -> Vec<String> {
        self.place.with(|place| place.roots.clone())
    }

    /// The git branch of the active folder.
    pub(crate) fn git_branch(&self) -> Option<String> {
        self.place.with(|place| place.git_branch.clone())
    }

    #[cfg(feature = "gui")]
    /// Every worktree of the active folder's repository except its primary checkout.
    pub(crate) fn worktrees(&self) -> Vec<Tree> {
        self.place.with(|place| place.worktrees.clone())
    }

    /// Follows the active chat into `workspace`.
    pub(crate) fn move_to(&self, workspace: &Path) {
        if workspace.as_os_str().is_empty() || self.place.peek().workspace == workspace {
            return;
        }
        self.place.set(Place::at(workspace));
        self.window.arrived(self);
    }
}

fn roots(workspace: &std::path::Path) -> Vec<String> {
    let real = std::fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    let mut roots = vec![real.display().to_string(), workspace.display().to_string()];
    roots.dedup();
    roots.retain(|root| root.len() > 1);
    roots
}
