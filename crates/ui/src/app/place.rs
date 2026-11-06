use std::path::{Path, PathBuf};

use super::{App, read_git_branch};

/// The folder the active chat works in, with its path spellings and git branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Place {
    pub(crate) workspace: PathBuf,
    pub(crate) roots: Vec<String>,
    pub(crate) git_branch: Option<String>,
}

impl Place {
    pub(crate) fn at(workspace: &Path) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
            roots: roots(workspace),
            git_branch: read_git_branch(workspace),
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

    /// Follows the active chat into `workspace`.
    pub(crate) fn move_to(&self, workspace: &Path) {
        if workspace.as_os_str().is_empty() || self.place.peek().workspace == workspace {
            return;
        }
        self.place.set(Place::at(workspace));
    }
}

fn roots(workspace: &std::path::Path) -> Vec<String> {
    let real = std::fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
    let mut roots = vec![real.display().to_string(), workspace.display().to_string()];
    roots.dedup();
    roots.retain(|root| root.len() > 1);
    roots
}
