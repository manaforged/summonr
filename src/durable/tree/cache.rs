use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::Snapshot;
use crate::durable::store::{self, Doc, Durability, Invalid, Json, Wait};

const CACHE_DIR: &str = "cache";
const TREES_DIR: &str = "trees";

pub(crate) fn cache_dir(home: &Path) -> PathBuf {
    home.join(CACHE_DIR).join(TREES_DIR)
}

fn remembered_at(cache: &Path, root: &Path) -> PathBuf {
    let key = summonr_core::text::fnv64(root.to_string_lossy().as_bytes());
    cache.join(format!("{key}.json"))
}

fn doc(path: PathBuf) -> Doc<Json<Option<Snapshot>>> {
    Doc::new(path, Durability::Renamed, Invalid::Fail, Wait::Never)
}

pub(crate) fn remembered(cache: &Path, root: &Path) -> Option<Snapshot> {
    doc(remembered_at(cache, root)).load().ok().flatten()
}

pub(crate) fn remember(cache: &Path, root: &Path, snapshot: &mut Snapshot) {
    if snapshot.hashed == 0 {
        return;
    }
    let path = remembered_at(cache, root);
    drop(doc(path.clone()).replace(&Some(snapshot.clone())));
    snapshot.hashed = 0;
    store::retain(cache, store::retention(), SystemTime::now(), |stale| {
        stale != path && std::fs::remove_file(stale).is_ok()
    });
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
