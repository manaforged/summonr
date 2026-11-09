use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::durable::log;
use crate::durable::store::{Doc, Durability, Invalid, Json, Wait};

pub(crate) struct Sidecar<T> {
    doc: Doc<Json<BTreeMap<String, T>>>,
    entries: BTreeMap<String, T>,
    dirty: bool,
}

impl<T: Serialize + DeserializeOwned> Sidecar<T> {
    pub(crate) fn load(home: &Path, file: &str) -> Self {
        let doc = Doc::new(
            log::sessions_dir(home).join(file),
            Durability::Renamed,
            Invalid::Fail,
            Wait::Forever,
        );
        let entries = doc.load().unwrap_or_default();
        Self {
            doc,
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
        if self.dirty || self.entries.len() != before {
            drop(self.doc.replace(&self.entries));
        }
    }
}

#[cfg(test)]
#[path = "sidecar_tests.rs"]
mod tests;
