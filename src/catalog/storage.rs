use std::any::Any;
use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::durable::store::{self, Doc, Durability, Encode, Invalid, Json, Wait};
use summonr_core::dirs::summonr_home;
use summonr_core::error::Result;

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
