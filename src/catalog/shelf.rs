use std::any::Any;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use serde::de::DeserializeOwned;

use crate::error::{Error, Result};
use crate::prompt::summonr_home;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

type Held = (Stamp, Arc<dyn Any + Send + Sync>);

static SHELF: Mutex<BTreeMap<PathBuf, Held>> = Mutex::new(BTreeMap::new());

fn stamp(path: &Path) -> std::io::Result<Stamp> {
    let meta = std::fs::metadata(path)?;
    Ok(Stamp {
        modified: meta.modified().ok(),
        len: meta.len(),
    })
}

pub(super) fn load<T>(file: &str) -> Result<Arc<T>>
where
    T: DeserializeOwned + Default + Send + Sync + 'static,
{
    let path = summonr_home()?.join(file);
    let now = match stamp(&path) {
        Ok(now) => now,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Arc::new(T::default()));
        }
        Err(error) => return Err(error.into()),
    };
    let hit = SHELF
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&path)
        .filter(|(held, _)| *held == now)
        .map(|(_, value)| Arc::clone(value));
    if let Some(value) = hit.and_then(|value| value.downcast::<T>().ok()) {
        return Ok(value);
    }
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Arc::new(T::default()));
        }
        Err(error) => return Err(error.into()),
    };
    let value: Arc<T> = Arc::new(
        serde_json::from_str(&raw)
            .map_err(|error| Error::Config(format!("{file} is corrupt: {error}")))?,
    );
    keep(&path, now, Arc::clone(&value));
    Ok(value)
}

pub(super) fn store<T: Send + Sync + 'static>(path: &Path, value: T) {
    if let Ok(now) = stamp(path) {
        keep(path, now, Arc::new(value));
    }
}

fn keep<T: Send + Sync + 'static>(path: &Path, now: Stamp, value: Arc<T>) {
    SHELF
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(path.to_path_buf(), (now, value));
}
