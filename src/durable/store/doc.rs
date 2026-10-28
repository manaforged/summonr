use std::any::Any;
use std::collections::BTreeSet;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;

use super::{Durability, forget, lock, memo, write};
use crate::config::tilde;
use crate::error::{Error, Result};

pub(crate) trait Codec {
    type Value;
    fn decode(path: &Path, text: &str) -> Result<Self::Value>;
}

pub(crate) trait Encode: Codec {
    fn encode(value: &Self::Value) -> Result<String>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Invalid {
    Fail,
    Warn,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum Wait {
    Forever,
    Within(fn() -> Duration),
}

pub(crate) struct Doc<C> {
    path: PathBuf,
    durability: Durability,
    invalid: Invalid,
    wait: Wait,
    codec: PhantomData<fn() -> C>,
}

pub(crate) struct PrettyJson<T>(PhantomData<fn() -> T>);

impl<T: Serialize + DeserializeOwned> Codec for PrettyJson<T> {
    type Value = T;

    fn decode(path: &Path, text: &str) -> Result<T> {
        serde_json::from_str(text)
            .map_err(|error| Error::Config(format!("{}: {error}", tilde(path))))
    }
}

impl<T: Serialize + DeserializeOwned> Encode for PrettyJson<T> {
    fn encode(value: &T) -> Result<String> {
        Ok(serde_json::to_string_pretty(value)?)
    }
}

impl<C: Codec> Doc<C>
where
    C::Value: Default,
{
    pub(crate) fn new(path: PathBuf, durability: Durability, invalid: Invalid, wait: Wait) -> Self {
        Self {
            path,
            durability,
            invalid,
            wait,
            codec: PhantomData,
        }
    }

    pub(crate) fn load(&self) -> Result<C::Value> {
        match (self.strict(), self.invalid) {
            (Err(error), Invalid::Warn) => {
                report(&self.path, &error);
                Ok(C::Value::default())
            }
            (outcome, _) => outcome,
        }
    }

    pub(crate) fn current(&self) -> Result<Arc<C::Value>>
    where
        C::Value: Any + Send + Sync,
    {
        memo(&self.path, || self.load())
    }

    pub(crate) fn update(&self, change: impl FnOnce(&mut C::Value) -> Result<bool>) -> Result<bool>
    where
        C: Encode,
    {
        let held = hold(&self.path, self.wait)?;
        let mut value = self.strict()?;
        let changed = change(&mut value)?;
        if changed {
            write(&self.path, C::encode(&value)?, self.durability)?;
            forget(&self.path);
        }
        drop(held);
        Ok(changed)
    }

    fn strict(&self) -> Result<C::Value> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => C::decode(&self.path, &text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(C::Value::default()),
            Err(error) => Err(error.into()),
        }
    }
}

pub(crate) fn hold(path: &Path, wait: Wait) -> Result<std::fs::File> {
    let limit = match wait {
        Wait::Forever => return lock(path, true)?.ok_or_else(|| locked(path, None)),
        Wait::Within(limit) => limit(),
    };
    if let Some(file) = lock(path, false)? {
        return Ok(file);
    }
    let (send, receive) = std::sync::mpsc::channel();
    let owned = path.to_path_buf();
    std::thread::spawn(move || drop(send.send(lock(&owned, true))));
    match receive.recv_timeout(limit) {
        Ok(held) => held?.ok_or_else(|| locked(path, Some(limit))),
        Err(_) => Err(locked(path, Some(limit))),
    }
}

fn locked(path: &Path, limit: Option<Duration>) -> Error {
    Error::Config(format!(
        "{} is locked by another summonr process; gave up after {}s",
        tilde(path),
        limit.map_or(0, |limit| limit.as_secs())
    ))
}

static REPORTED: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

fn report(path: &Path, error: &Error) {
    let fresh = REPORTED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(path.to_path_buf());
    if fresh {
        eprintln!(
            "summonr: {} is unreadable ({error}); summonr leaves it as it is until you fix or remove it",
            tilde(path)
        );
    }
}
