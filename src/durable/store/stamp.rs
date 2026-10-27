use std::any::{Any, TypeId};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use crate::error::Result;

pub(crate) type Stamp = Option<(SystemTime, u64)>;

struct Entry {
    stamp: Stamp,
    taken: SystemTime,
    value: Arc<dyn Any + Send + Sync>,
}

static CACHE: RwLock<BTreeMap<(PathBuf, TypeId), Entry>> = RwLock::new(BTreeMap::new());

pub(crate) fn stamp(path: &Path) -> Stamp {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

pub(crate) fn settled(stamp: Stamp, taken: SystemTime) -> bool {
    stamp.is_none_or(|(modified, _)| {
        modified
            .checked_add(crate::limits::limits().stamp_settle())
            .is_some_and(|settles| settles <= taken)
    })
}

pub(crate) fn memo<T: Any + Send + Sync>(
    path: &Path,
    load: impl FnOnce() -> Result<T>,
) -> Result<Arc<T>> {
    let taken = SystemTime::now();
    let now = stamp(path);
    let key = (path.to_path_buf(), TypeId::of::<T>());
    let hit = CACHE.read().ok().and_then(|cache| {
        cache
            .get(&key)
            .filter(|entry| entry.stamp == now && settled(entry.stamp, entry.taken))
            .and_then(|entry| Arc::clone(&entry.value).downcast::<T>().ok())
    });
    if let Some(hit) = hit {
        return Ok(hit);
    }
    let value = Arc::new(load()?);
    let entry = Entry {
        stamp: now,
        taken,
        value: Arc::clone(&value) as Arc<dyn Any + Send + Sync>,
    };
    drop(CACHE.write().map(|mut cache| cache.insert(key, entry)));
    Ok(value)
}

pub(crate) fn forget(path: &Path) {
    drop(
        CACHE
            .write()
            .map(|mut cache| cache.retain(|(file, _), _| file != path)),
    );
}
