//! Legacy `.mage/plan.md` path. Product Plan revisions live in the journal.

use std::path::{Path, PathBuf};

#[cfg(test)]
use crate::error::Result;

/// Location of a legacy `.mage/plan.md` import source.
#[must_use]
pub(crate) fn path(workspace: &Path) -> PathBuf {
    workspace.join(".mage/plan.md")
}

#[cfg(test)]
pub(crate) fn write(workspace: &Path, text: &str) -> Result<()> {
    let dest = path(workspace);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(std::fs::write(&dest, text)?)
}

#[cfg(test)]
pub(crate) fn clear(workspace: &Path) -> Result<bool> {
    let dest = path(workspace);
    if dest.is_file() {
        std::fs::remove_file(&dest)?;
        return Ok(true);
    }
    Ok(false)
}

#[cfg(test)]
#[must_use]
pub(crate) fn read(workspace: &Path) -> Option<String> {
    std::fs::read_to_string(path(workspace)).ok()
}

#[cfg(test)]
#[path = "plan_tests.rs"]
mod tests;
