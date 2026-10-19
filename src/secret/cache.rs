use std::path::{Path, PathBuf};
use std::sync::RwLock;
use std::time::SystemTime;

use toml_edit::DocumentMut;

pub(super) type Stamp = Option<(SystemTime, u64)>;

struct Cached {
    file: PathBuf,
    stamp: Stamp,
    document: DocumentMut,
}

static DECRYPTED: RwLock<Option<Cached>> = RwLock::new(None);

pub(super) fn stamp(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

pub(super) fn settled(stamp: Stamp, taken: SystemTime) -> bool {
    stamp.is_none_or(|(modified, _)| {
        modified
            .checked_add(crate::limits::limits().stamp_settle())
            .is_some_and(|settles| settles <= taken)
    })
}

pub(super) fn find<T>(
    file: &Path,
    stamp: Stamp,
    read: impl FnOnce(&DocumentMut) -> T,
) -> Option<T> {
    let cached = DECRYPTED.read().ok()?;
    cached
        .as_ref()
        .filter(|entry| entry.file == file && entry.stamp == stamp)
        .map(|entry| read(&entry.document))
}

pub(super) fn keep(file: &Path, stamp: Stamp, document: &DocumentMut) {
    drop(DECRYPTED.write().map(|mut cached| {
        *cached = Some(Cached {
            file: file.to_path_buf(),
            stamp,
            document: document.clone(),
        });
    }));
}

pub(super) fn clear() {
    drop(DECRYPTED.write().map(|mut cached| *cached = None));
}
