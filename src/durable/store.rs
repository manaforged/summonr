use std::fs::{File, OpenOptions, Permissions, TryLockError};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tempfile::NamedTempFile;

pub(crate) struct Staged<'a> {
    temp: NamedTempFile,
    path: &'a Path,
    durability: Durability,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Durability {
    Synced,
    Renamed,
}

pub(crate) fn write(path: &Path, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    prepare(&target, bytes.as_ref(), false, Durability::Synced)?.commit()
}

pub(crate) fn read_or_empty(path: &Path) -> io::Result<String> {
    match std::fs::read_to_string(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        other => other,
    }
}

#[cfg(unix)]
pub(crate) fn scratch(path: &Path, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(path)?
        .write_all(bytes.as_ref())
}

pub(crate) fn keep(dir: &Path, prefix: &str, suffix: &str, bytes: &[u8]) -> io::Result<PathBuf> {
    let mut file = tempfile::Builder::new()
        .prefix(prefix)
        .suffix(suffix)
        .tempfile_in(dir)?;
    file.write_all(bytes)?;
    file.keep()
        .map(|(_, path)| path)
        .map_err(|error| error.error)
}

pub(crate) fn lock_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".lock");
    PathBuf::from(name)
}

pub fn lock(path: &Path, wait: bool) -> io::Result<Option<File>> {
    let name = lock_path(path);
    std::fs::create_dir_all(parent(&name))?;
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&name)?;
    if wait {
        file.lock()?;
        return Ok(Some(file));
    }
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(error)) => Err(error),
    }
}

pub(crate) fn stage<'a>(path: &'a Path, bytes: &[u8], keep_mode: bool) -> io::Result<Staged<'a>> {
    prepare(path, bytes, keep_mode, Durability::Renamed)
}

fn prepare<'a>(
    path: &'a Path,
    bytes: &[u8],
    keep_mode: bool,
    durability: Durability,
) -> io::Result<Staged<'a>> {
    let parent = parent(path);
    std::fs::create_dir_all(parent)?;
    let mut temp = NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    if keep_mode && let Some(permissions) = mode(path)? {
        temp.as_file().set_permissions(permissions)?;
    }
    if durability == Durability::Synced {
        temp.as_file().sync_all()?;
    }
    Ok(Staged {
        temp,
        path,
        durability,
    })
}

pub(crate) fn rename(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(parent(to))?;
    std::fs::rename(from, to)?;
    sync_dir(to)
}

impl Staged<'_> {
    pub(crate) fn commit(self) -> io::Result<()> {
        self.temp.persist(self.path).map_err(|error| error.error)?;
        match self.durability {
            Durability::Synced => sync_dir(self.path),
            Durability::Renamed => Ok(()),
        }
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

#[cfg(all(test, unix))]
#[path = "store_tests.rs"]
mod tests;
