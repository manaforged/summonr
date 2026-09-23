use std::fs::Permissions;
use std::io::{self, Write};
use std::path::Path;

use tempfile::NamedTempFile;

pub(crate) struct Staged<'a> {
    temp: NamedTempFile,
    path: &'a Path,
}

pub(crate) fn write(path: &Path, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    stage(path, bytes.as_ref(), false)?.commit()
}

pub(crate) fn stage<'a>(path: &'a Path, bytes: &[u8], keep_mode: bool) -> io::Result<Staged<'a>> {
    let parent = parent(path);
    std::fs::create_dir_all(parent)?;
    let mut temp = NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    if keep_mode && let Some(permissions) = mode(path)? {
        temp.as_file().set_permissions(permissions)?;
    }
    temp.as_file().sync_all()?;
    Ok(Staged { temp, path })
}

pub(crate) fn rename(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(parent(to))?;
    std::fs::rename(from, to)?;
    sync_dir(to)
}

impl Staged<'_> {
    pub(crate) fn commit(self) -> io::Result<()> {
        self.temp.persist(self.path).map_err(|error| error.error)?;
        sync_dir(self.path)
    }
}

fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn mode(path: &Path) -> io::Result<Option<Permissions>> {
    match std::fs::metadata(path) {
        Ok(metadata) => Ok(Some(metadata.permissions())),
        #[cfg(unix)]
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Ok(Some(std::os::unix::fs::PermissionsExt::from_mode(0o644)))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn sync_dir(path: &Path) -> io::Result<()> {
    if cfg!(unix) {
        std::fs::File::open(parent(path))?.sync_all()?;
    }
    Ok(())
}
