use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::{Error, Issued, Parts, Result, Store};

const LIFETIME_CEILING: u64 = 10 * 365 * 24 * 3600;

#[derive(Deserialize, Serialize)]
struct Stored {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_at: Option<u64>,
    #[serde(default)]
    id_token: Option<String>,
}

pub(crate) fn parse_stored(raw: &str) -> Option<Issued> {
    let disk: Stored = serde_json::from_str(raw).ok()?;
    Some(Issued {
        access: disk.access_token,
        refresh: disk.refresh_token,
        expires: disk.expires_at.map(from_unix),
        identity: disk.id_token,
    })
}

fn to_stored(issued: &Issued) -> Stored {
    Stored {
        access_token: issued.access.clone(),
        refresh_token: issued.refresh.clone(),
        expires_at: issued.expires.map(to_unix),
        id_token: issued.identity.clone(),
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

pub(super) fn from_unix(at: u64) -> Instant {
    Instant::now() + crate::seconds(at.saturating_sub(now_unix()).min(LIFETIME_CEILING))
}

fn to_unix(at: Instant) -> u64 {
    now_unix() + at.saturating_duration_since(Instant::now()).as_secs()
}

pub(super) fn lifetime(seconds: u64) -> Instant {
    Instant::now() + crate::seconds(seconds.min(LIFETIME_CEILING))
}

pub(super) fn stored(parts: &Parts<'_>, store: &dyn Store) -> Result<Option<Issued>> {
    Ok(store.get(parts.name)?.as_deref().and_then(parse_stored))
}

pub(super) fn held(parts: &Parts<'_>) -> Result<Option<Issued>> {
    let slot = parts
        .held
        .lock()
        .map_err(|_| Error::Config("the token cache was poisoned".to_string()))?;
    Ok(slot.as_ref().filter(|issued| issued.fresh()).cloned())
}

pub(super) fn keep(parts: &Parts<'_>, issued: Issued) -> Result<()> {
    let mut slot = parts
        .held
        .lock()
        .map_err(|_| Error::Config("the token cache was poisoned".to_string()))?;
    slot.replace(issued);
    Ok(())
}

pub(super) fn save(parts: &Parts<'_>, store: &dyn Store, issued: Issued) -> Result<()> {
    let raw = serde_json::to_string(&to_stored(&issued))
        .map_err(|error| Error::Config(error.to_string()))?;
    store.set(parts.name, &raw)?;
    keep(parts, issued)
}
