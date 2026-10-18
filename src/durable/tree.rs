use std::collections::BTreeMap;
use std::fs::{File, Metadata, read_link};
use std::hash::{Hash, Hasher};
use std::io::{self, Read};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::text::Fnv64;

#[derive(Clone, Copy, Debug, Deserialize, Hash, PartialEq, Eq, Serialize)]
pub(crate) struct Entry(u64, u64, u64);

const WALK_LIMIT: usize = 20_000;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct Snapshot {
    #[serde(default, with = "shared")]
    files: Arc<BTreeMap<String, Entry>>,
    #[serde(default)]
    taken: u64,
    #[serde(default, with = "shared")]
    stamps: Arc<BTreeMap<String, Stamp>>,
    #[serde(skip)]
    hashed: usize,
}

impl PartialEq for Snapshot {
    fn eq(&self, other: &Self) -> bool {
        self.files == other.files
    }
}

impl Eq for Snapshot {}

impl Snapshot {
    pub(crate) fn from_files(files: BTreeMap<String, Entry>) -> Self {
        Self {
            files: Arc::new(files),
            taken: 0,
            stamps: Arc::default(),
            hashed: 0,
        }
    }
}

impl Hash for Snapshot {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.files.hash(state);
    }
}

pub(crate) fn snapshot(
    root: &Path,
    exclude: &[PathBuf],
    previous: Option<&Snapshot>,
) -> io::Result<Snapshot> {
    let taken = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut files = BTreeMap::new();
    let mut stamps = BTreeMap::new();
    let mut hashed = 0;
    let own = own_outputs();
    for relative in listing(root)? {
        let path = root.join(&relative);
        // complexity: bounded by the summonr state directories the driver excludes
        if exclude.iter().any(|exclude| path.starts_with(exclude)) {
            continue;
        }
        let name = relative.to_string_lossy().replace('\\', "/");
        if crate::policy::workspace_control(&relative) || crate::diff::ignored::hidden(&name) {
            continue;
        }
        if let Some((entry, stamp, fresh)) = mark(&path, &name, previous, &own)? {
            hashed += usize::from(fresh);
            stamps.insert(name.clone(), stamp);
            files.insert(name, entry);
        }
    }
    Ok(Snapshot {
        files: Arc::new(files),
        taken,
        stamps: Arc::new(stamps),
        hashed,
    })
}

fn mark(
    path: &Path,
    name: &str,
    previous: Option<&Snapshot>,
    own: &[Own],
) -> io::Result<Option<(Entry, Stamp, bool)>> {
    let metadata = match path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) if skippable(&error) => return Ok(None),
        Err(error) => return Err(at(path, error)),
    };
    if !metadata.is_file() && !metadata.is_symlink() {
        return Ok(None);
    }
    if is_own(path, &metadata, own) {
        return Ok(None);
    }
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .unwrap_or_default()
        .as_secs();
    let stamp = Stamp {
        mode: mode(&metadata),
        changed: changed_at(&metadata),
    };
    let reused = previous
        .filter(|_| metadata.is_file())
        .and_then(|previous| previous.unchanged(name, metadata.len(), modified, stamp));
    let mark = match reused {
        Some(mark) => mark,
        None => match fingerprint(path, &metadata) {
            Ok(mark) => mark,
            Err(error) if skippable(&error) => return Ok(None),
            Err(error) => return Err(at(path, error)),
        },
    };
    Ok(Some((
        Entry(metadata.len(), modified, mark),
        stamp,
        reused.is_none(),
    )))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
struct Stamp {
    mode: u32,
    changed: (i64, i64),
}

#[cfg(unix)]
fn changed_at(metadata: &Metadata) -> (i64, i64) {
    use std::os::unix::fs::MetadataExt;
    (metadata.ctime(), metadata.ctime_nsec())
}

#[cfg(not(unix))]
fn changed_at(_: &Metadata) -> (i64, i64) {
    (0, 0)
}

#[cfg(unix)]
fn mode(metadata: &Metadata) -> u32 {
    metadata.permissions().mode()
}

#[cfg(not(unix))]
fn mode(metadata: &Metadata) -> u32 {
    u32::from(metadata.permissions().readonly())
}

fn fingerprint(path: &Path, metadata: &Metadata) -> io::Result<u64> {
    let mut hasher = Fnv64::default();
    hasher.write_u8(u8::from(metadata.is_symlink()));
    hasher.write_u32(mode(metadata));
    if metadata.is_symlink() {
        hasher.write(read_link(path)?.as_os_str().as_encoded_bytes());
        return Ok(hasher.finish());
    }
    let mut file = File::open(path)?;
    let mut buffer = [0; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            return Ok(hasher.finish());
        }
        hasher.write(&buffer[..read]);
    }
}

impl Snapshot {
    fn unchanged(&self, name: &str, len: u64, modified: u64, stamp: Stamp) -> Option<u64> {
        let Entry(was_len, was_modified, mark) = *self.files.get(name)?;
        (was_len == len
            && was_modified == modified
            && modified < self.taken
            && self.stamps.get(name) == Some(&stamp))
        .then_some(mark)
    }
}

pub(crate) fn changed(base: &Snapshot, capture: &Snapshot) -> Vec<String> {
    let mut paths: Vec<String> = capture
        .files
        .iter()
        .filter(|(path, entry)| base.files.get(*path) != Some(*entry))
        .map(|(path, _)| path.clone())
        .collect();
    paths.extend(
        base.files
            .keys()
            .filter(|path| !capture.files.contains_key(*path))
            .cloned(),
    );
    paths.retain(|path| !crate::diff::ignored::hidden(path));
    paths.sort();
    paths
}

pub(crate) fn listing(root: &Path) -> io::Result<Vec<PathBuf>> {
    if crate::diff::inside_git(root)
        && let Ok(output) = crate::diff::git_output(
            root,
            &[],
            &[
                "ls-files",
                "-z",
                "--cached",
                "--others",
                "--exclude-standard",
            ],
            None,
        )
    {
        return Ok(output
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
            .map(git_path)
            .collect());
    }
    let mut found = Vec::new();
    let device = std::fs::metadata(root).ok().as_ref().and_then(device_of);
    walk(root, root, device, &mut found)?;
    Ok(found)
}

#[cfg(unix)]
fn git_path(name: &[u8]) -> PathBuf {
    PathBuf::from(std::ffi::OsStr::from_bytes(name))
}

#[cfg(not(unix))]
fn git_path(name: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(name).into_owned())
}

fn skippable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
    ) || locked(error)
}

#[cfg(windows)]
fn locked(error: &io::Error) -> bool {
    use windows_sys::Win32::Foundation::{ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION};
    error
        .raw_os_error()
        .and_then(|code| u32::try_from(code).ok())
        .is_some_and(|code| [ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION].contains(&code))
}

#[cfg(not(windows))]
fn locked(_: &io::Error) -> bool {
    false
}

fn at(path: &Path, error: io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{}: {error}", path.display()))
}

fn walk(root: &Path, dir: &Path, device: Option<u64>, found: &mut Vec<PathBuf>) -> io::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) if dir != root => return Ok(()),
        Err(error) => return Err(at(dir, error)),
    };
    for entry in entries.flatten() {
        if found.len() >= WALK_LIMIT {
            return Ok(());
        }
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            if descend(&entry, device) {
                walk(root, &path, device, found)?;
            }
        } else if (kind.is_file() || kind.is_symlink())
            && let Ok(relative) = path.strip_prefix(root)
        {
            found.push(relative.to_path_buf());
        }
    }
    Ok(())
}

fn descend(entry: &std::fs::DirEntry, device: Option<u64>) -> bool {
    let name = entry.file_name();
    !crate::SKIP_DIRS.iter().any(|skip| name == *skip)
        && device.is_none_or(|device| {
            entry
                .metadata()
                .is_ok_and(|metadata| device_of(&metadata) == Some(device))
        })
}

#[cfg(unix)]
fn device_of(metadata: &Metadata) -> Option<u64> {
    Some(std::os::unix::fs::MetadataExt::dev(metadata))
}

#[cfg(not(unix))]
fn device_of(_: &Metadata) -> Option<u64> {
    None
}

mod cache;
mod own;

use cache::shared;
pub(crate) use cache::{cache_dir, remember, remembered};

#[cfg(all(test, unix))]
use own::identity;
pub(crate) use own::own_files;
use own::{Own, is_own, own_outputs};

#[cfg(all(test, unix))]
#[path = "tree_tests.rs"]
mod tests;

#[cfg(all(test, windows))]
#[path = "tree_windows_tests.rs"]
mod windows_tests;
