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
use summonr_core::error::{Error, Result};

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
    Never,
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

pub(crate) struct Json<T>(PhantomData<fn() -> T>);

pub(crate) struct Toml<T>(PhantomData<fn() -> T>);

pub(crate) struct Lines;

impl Codec for Lines {
    type Value = Vec<String>;

    fn decode(_: &Path, text: &str) -> Result<Vec<String>> {
        Ok(text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect())
    }
}

impl Encode for Lines {
    fn encode(value: &Vec<String>) -> Result<String> {
        Ok(value.iter().map(|line| format!("{line}\n")).collect())
    }
}

impl<T: Serialize + DeserializeOwned> Codec for Json<T> {
    type Value = T;

    fn decode(path: &Path, text: &str) -> Result<T> {
        serde_json::from_str(text)
            .map_err(|error| Error::Config(format!("{}: {error}", tilde(path))))
    }
}

impl<T: Serialize + DeserializeOwned> Encode for Json<T> {
    fn encode(value: &T) -> Result<String> {
        Ok(serde_json::to_string(value)?)
    }
}

impl<T: Serialize + DeserializeOwned> Codec for Toml<T> {
    type Value = T;

    fn decode(path: &Path, text: &str) -> Result<T> {
        toml::from_str(text).map_err(|error| Error::Config(format!("{}: {error}", tilde(path))))
    }
}

impl<T: Serialize + DeserializeOwned> Encode for Toml<T> {
    fn encode(value: &T) -> Result<String> {
        toml::to_string(value).map_err(|error| Error::Config(error.to_string()))
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
        let held = self.hold()?;
        let mut value = self.strict()?;
        let changed = change(&mut value)?;
        if changed {
            write(&self.path, C::encode(&value)?, self.durability)?;
            forget(&self.path);
        }
        drop(held);
        Ok(changed)
    }

    pub(crate) fn replace(&self, value: &C::Value) -> Result<()>
    where
        C: Encode,
    {
        let held = self.hold()?;
        write(&self.path, C::encode(value)?, self.durability)?;
        forget(&self.path);
        drop(held);
        Ok(())
    }

    fn hold(&self) -> Result<Option<std::fs::File>> {
        match self.wait {
            Wait::Never => Ok(None),
            wait => hold(&self.path, wait).map(Some),
        }
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
        Wait::Never => return lock(path, false)?.ok_or_else(|| locked(path, None)),
        Wait::Forever => return lock(path, true)?.ok_or_else(|| locked(path, None)),
        Wait::Within(limit) => limit(),
    };
    super::lock_within(path, limit)?.ok_or_else(|| locked(path, Some(limit)))
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
