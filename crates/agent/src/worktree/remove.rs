use std::path::Path;

use crate::durable::{driver, store};
use summonr_core::error::{Error, Result};

use super::{Tree, base, worktree_id};

/// Removes the worktree and its branch, with any work on them.
pub(crate) fn throw_away(tree: &Tree, own: &str) -> Result<()> {
    if let Some(session) = tree.holder(own)? {
        return Err(Error::Config(format!(
            "agent {} still uses this worktree; stop it first",
            crate::session::short(&session)
        )));
    }
    tree.delete()
}

impl Tree {
    pub(crate) fn remove(&self) -> Result<()> {
        self.check_merged()?;
        self.delete()
    }

    fn delete(&self) -> Result<()> {
        let head = crate::diff::git_env(&self.primary, &[], &["rev-parse", &self.branch])?;
        let path = self.path.to_str().ok_or_else(|| {
            Error::Config(format!(
                "worktree path is not utf-8: {}",
                self.path.display()
            ))
        })?;
        crate::diff::git_env(&self.primary, &[], &["worktree", "remove", "--force", path])?;
        let reference = format!("refs/heads/{}", self.branch);
        crate::diff::git_env(&self.primary, &[], &["update-ref", "-d", &reference, &head])
            .map_err(|error| {
                Error::Config(format!(
                    "removed {path} but branch {} remains at {head}; delete it with `git branch -d {}`: {error}",
                    self.branch, self.branch
                ))
            })?;
        if let Some(id) = worktree_id(&self.branch)
            && let Ok(record) = base::record(&self.primary, id)
        {
            drop(std::fs::remove_file(record));
        }
        if let Some(parent) = self.path.parent() {
            drop(std::fs::remove_dir(parent));
        }
        Ok(())
    }

    fn check_merged(&self) -> Result<()> {
        let contains = |target: &str| {
            crate::diff::git_env(
                &self.primary,
                &[],
                &["merge-base", "--is-ancestor", &self.branch, target],
            )
            .is_ok()
        };
        if contains("HEAD") {
            return Ok(());
        }
        let base = self.known_base()?;
        if contains(&base) {
            return Ok(());
        }
        let range = format!("{base}..{}", self.branch);
        let count = crate::diff::git_env(&self.primary, &[], &["rev-list", "--count", &range])?;
        Err(Error::Config(format!(
            "{} has commits {base} lacks ({count}); merge it first",
            self.branch
        )))
    }

    pub(crate) fn holder(&self, own: &str) -> Result<Option<String>> {
        let here = driver::canonical(&self.path);
        for info in crate::session::all()?.into_iter().rev() {
            if info.id == own {
                continue;
            }
            let Some(cwd) = info.cwd.as_deref() else {
                continue;
            };
            if driver::canonical(Path::new(cwd)) != here {
                continue;
            }
            if store::lock(&info.path, false)?.is_none() {
                return Ok(Some(info.id));
            }
        }
        Ok(None)
    }
}
