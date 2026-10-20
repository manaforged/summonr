use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::durable::log;
use crate::durable::store;

pub(crate) struct Sidecar<T> {
    path: PathBuf,
    entries: BTreeMap<String, T>,
    dirty: bool,
}

impl<T: Serialize + DeserializeOwned> Sidecar<T> {
    pub(crate) fn load(home: &Path, file: &str) -> Self {
        let path = log::sessions_dir(home).join(file);
        let entries = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self {
            path,
            entries,
            dirty: false,
        }
    }

    pub(crate) fn get(&self, id: &str) -> Option<&T> {
        self.entries.get(id)
    }

    pub(crate) fn put(&mut self, id: &str, value: T) {
        self.entries.insert(id.to_string(), value);
        self.dirty = true;
    }

    pub(crate) fn forget(&mut self, id: &str) {
        self.dirty |= self.entries.remove(id).is_some();
    }

    pub(crate) fn save<'a>(mut self, live: impl IntoIterator<Item = &'a str>) {
        let live: BTreeSet<&str> = live.into_iter().collect();
        let before = self.entries.len();
        self.entries.retain(|id, _| live.contains(id.as_str()));
        if !self.dirty && self.entries.len() == before {
            return;
        }
        if let Ok(bytes) = serde_json::to_vec(&self.entries) {
            drop(store::write(&self.path, bytes, store::Durability::Renamed));
        }
    }
}

#[cfg(test)]
#[path = "sidecar_tests.rs"]
mod tests;
