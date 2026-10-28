use std::any::Any;
use std::marker::PhantomData;
use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::durable::store::{self, Codec, Doc, Durability, Encode, Invalid, Wait};
use crate::error::{Error, Result};
use crate::prompt::summonr_home;

pub(super) struct Json<T>(PhantomData<fn() -> T>);

impl<T: Serialize + DeserializeOwned> Codec for Json<T> {
    type Value = T;

    fn decode(path: &Path, text: &str) -> Result<T> {
        let file = path.file_name().unwrap_or_default().to_string_lossy();
        serde_json::from_str(text)
            .map_err(|error| Error::Config(format!("{file} is corrupt: {error}")))
    }
}

impl<T: Serialize + DeserializeOwned> Encode for Json<T> {
    fn encode(value: &T) -> Result<String> {
        Ok(serde_json::to_string(value)?)
    }
}

pub(super) fn doc<T: Serialize + DeserializeOwned + Default>(file: &str) -> Result<Doc<Json<T>>> {
    Ok(Doc::new(
        summonr_home()?.join(file),
        Durability::Renamed,
        Invalid::Fail,
        Wait::Forever,
    ))
}

pub(super) fn load<T>(file: &str) -> Result<Arc<T>>
where
    T: Serialize + DeserializeOwned + Default + Any + Send + Sync,
{
    doc::<T>(file)?.current()
}

pub(super) fn save<T: Serialize + DeserializeOwned>(file: &str, value: &T) -> Result<()> {
    let path = summonr_home()?.join(file);
    store::write(&path, Json::<T>::encode(value)?, Durability::Renamed)?;
    store::forget(&path);
    Ok(())
}
