use std::path::{Path, PathBuf};

use crate::durable::store;
use summonr_core::error::{Error, Result};

use super::{Tree, common_dir, worktree_id};

pub(super) fn record(primary: &Path, id: &str) -> Result<PathBuf> {
    Ok(common_dir(primary)?
        .join("summon")
        .join(format!("{id}.base")))
}

pub(super) fn recorded(primary: &Path, branch: &str) -> Option<String> {
    let path = record(primary, worktree_id(branch)?).ok()?;
    let text = std::fs::read_to_string(path).ok()?;
    let base = text.trim();
    (!base.is_empty()).then(|| base.to_string())
}

pub(super) fn save(primary: &Path, id: &str, base: &str) -> Result<PathBuf> {
    let path = record(primary, id)?;
    store::write(&path, format!("{base}\n"), store::Durability::Synced)?;
    Ok(path)
}

impl Tree {
    pub(crate) fn known_base(&self) -> Result<String> {
        if !self.base.is_empty() {
            return Ok(self.base.clone());
        }
        let fork = self.fork_point().map_or_else(
            |error| format!("git cannot find its fork point: {error}"),
            |fork| format!("it forked from {fork}"),
        );
        Err(Error::Config(format!(
            "{} has no recorded base, so summonr will not guess the branch to merge into ({fork}); check out that branch in {} and run `git merge --ff-only {}`",
            self.branch,
            self.primary.display(),
            self.branch
        )))
    }

    pub(crate) fn fork_point(&self) -> Result<String> {
        crate::diff::git_env(&self.primary, &[], &["merge-base", "HEAD", &self.branch])
    }
}
