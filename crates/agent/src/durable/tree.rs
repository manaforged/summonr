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

use summonr_core::text::Fnv64;

#[derive(Clone, Copy, Debug, Deserialize, Hash, PartialEq, Eq, Serialize)]
pub(crate) struct Entry(u64, u64, u64);

const HASH_LIMIT: u64 = 64 << 10;
const UNREAD: u64 = 0;

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

/// The workspace's files as of now. A Git workspace lists through Git; any other folder is never
/// walked and covers only the files `previous` knew and the paths in `scope` the session touched.
pub(crate) fn snapshot(
    root: &Path,
    exclude: &[PathBuf],
    previous: Option<&Snapshot>,
    scope: &[String],
) -> io::Result<Snapshot> {
    let walk = || {
        let taken = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let mut files = BTreeMap::new();
        let mut stamps = BTreeMap::new();
        let mut hashed = 0;
        let own = own_outputs();
        for relative in listing(root, previous, scope) {
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
    };
    let threaded = tokio::runtime::Handle::try_current()
        .is_ok_and(|handle| handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread);
    if threaded {
        tokio::task::block_in_place(walk)
    } else {
        walk()
    }
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
            Err(error) if locked(&error) => previous
                .and_then(|previous| previous.known(name, metadata.len(), modified, stamp))
                .unwrap_or(UNREAD),
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

fn modified_nanos(metadata: &Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .unwrap_or_default()
        .as_nanos()
}

fn fingerprint(path: &Path, metadata: &Metadata) -> io::Result<u64> {
    let mut hasher = Fnv64::default();
    hasher.write_u8(u8::from(metadata.is_symlink()));
    hasher.write_u32(mode(metadata));
    if metadata.is_symlink() {
        hasher.write(read_link(path)?.as_os_str().as_encoded_bytes());
        return Ok(hasher.finish());
    }
    if metadata.len() > HASH_LIMIT {
        hasher.write_u64(metadata.len());
        hasher.write_u128(modified_nanos(metadata));
        hasher.write_i64(changed_at(metadata).0);
        hasher.write_i64(changed_at(metadata).1);
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
        self.known(name, len, modified, stamp)
            .filter(|mark| modified < self.taken || *mark == UNREAD)
    }

    fn known(&self, name: &str, len: u64, modified: u64, stamp: Stamp) -> Option<u64> {
        let Entry(was_len, was_modified, mark) = *self.files.get(name)?;
        (was_len == len && was_modified == modified && self.stamps.get(name) == Some(&stamp))
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

fn listing(root: &Path, previous: Option<&Snapshot>, scope: &[String]) -> Vec<PathBuf> {
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
        return output
            .split(|byte| *byte == 0)
            .filter(|name| !name.is_empty())
            .map(git_path)
            .collect();
    }
    let known = previous
        .into_iter()
        .flat_map(|previous| previous.files.keys());
    let paths: std::collections::BTreeSet<&String> = known.chain(scope).collect();
    paths.into_iter().map(PathBuf::from).collect()
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

pub(crate) mod base;
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
