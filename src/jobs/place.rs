use std::path::{Path, PathBuf};

use crate::error::Result;

pub(crate) fn dir(workspace: &Path) -> PathBuf {
    let local = workspace.join(".summonr").join("jobs");
    if local.is_dir() {
        return local;
    }
    spare(workspace)
        .filter(|spare| spare.is_dir())
        .unwrap_or(local)
}

fn spare(workspace: &Path) -> Option<PathBuf> {
    let home = crate::prompt::summonr_home().ok()?;
    Some(
        home.join("jobs")
            .join(crate::mail::slug(&workspace.display().to_string())),
    )
}

pub(super) fn made(workspace: &Path) -> Result<PathBuf> {
    let root = dir(workspace);
    match std::fs::create_dir_all(&root) {
        Ok(()) => Ok(root),
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
            ) =>
        {
            let spare = spare(workspace).ok_or(error)?;
            std::fs::create_dir_all(&spare)?;
            Ok(spare)
        }
        Err(error) => Err(error.into()),
    }
}
