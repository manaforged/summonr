use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::Snapshot;

const CACHE_DIR: &str = "cache";
const TREES_DIR: &str = "trees";

pub(crate) fn cache_dir(home: &Path) -> PathBuf {
    home.join(CACHE_DIR).join(TREES_DIR)
}

fn remembered_at(cache: &Path, root: &Path) -> PathBuf {
    let key = crate::text::fnv64(root.to_string_lossy().as_bytes());
    cache.join(format!("{key}.json"))
}

pub(crate) fn remembered(cache: &Path, root: &Path) -> Option<Snapshot> {
    serde_json::from_slice(&std::fs::read(remembered_at(cache, root)).ok()?).ok()
}

pub(crate) fn remember(cache: &Path, root: &Path, snapshot: &mut Snapshot) {
    if snapshot.hashed == 0 {
        return;
    }
    let path = remembered_at(cache, root);
    if let Ok(bytes) = serde_json::to_vec(snapshot) {
        drop(crate::durable::store::write(
            &path,
            bytes,
            crate::durable::store::Durability::Renamed,
        ));
    }
    snapshot.hashed = 0;
    let retain = crate::config::file().map_or(crate::limits::limits().session_retain(), |file| {
        file.sessions.retain()
    });
    prune(cache, &path, retain, SystemTime::now());
}

pub(super) fn prune(cache: &Path, keep: &Path, retain: Duration, now: SystemTime) {
    let Ok(entries) = std::fs::read_dir(cache) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let stale = path != keep
            && entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .is_ok_and(|at| now.duration_since(at).is_ok_and(|age| age > retain));
        if stale {
            drop(std::fs::remove_file(path));
        }
    }
}

pub(super) mod shared {
    use std::sync::Arc;

    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(in crate::durable::tree) fn serialize<T: Serialize, S: Serializer>(
        value: &Arc<T>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value.as_ref().serialize(serializer)
    }

    pub(in crate::durable::tree) fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Arc<T>, D::Error> {
        T::deserialize(deserializer).map(Arc::new)
    }
}
