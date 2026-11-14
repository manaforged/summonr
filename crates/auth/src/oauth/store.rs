use std::time::Instant;

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
    #[serde(default)]
    client_id: Option<String>,
}

pub(crate) fn parse_stored(raw: &str) -> Option<Issued> {
    let disk: Stored = serde_json::from_str(raw).ok()?;
    Some(Issued {
        access: disk.access_token,
        refresh: disk.refresh_token,
        expires: disk.expires_at.map(from_unix),
        identity: disk.id_token,
        client: disk.client_id,
    })
}

fn to_stored(issued: &Issued) -> Stored {
    Stored {
        access_token: issued.access.clone(),
        refresh_token: issued.refresh.clone(),
        expires_at: issued.expires.map(to_unix),
        id_token: issued.identity.clone(),
        client_id: issued.client.clone(),
    }
}

pub(super) fn from_unix(at: u64) -> Instant {
    Instant::now()
        + std::time::Duration::from_secs(
            at.saturating_sub(summonr_core::clock::now())
                .min(LIFETIME_CEILING),
        )
}

fn to_unix(at: Instant) -> u64 {
    summonr_core::clock::now() + at.saturating_duration_since(Instant::now()).as_secs()
}

pub(super) fn lifetime(seconds: u64) -> Instant {
    Instant::now() + std::time::Duration::from_secs(seconds.min(LIFETIME_CEILING))
}

pub(super) fn stored(parts: &Parts<'_>, store: &dyn Store) -> Result<Option<Issued>> {
    Ok(store.get(parts.name)?.as_deref().and_then(parse_stored))
}

pub(crate) fn stale(parts: &Parts<'_>, store: &dyn Store) -> Result<()> {
    match stored(parts, store)? {
        Some(grant) => expire(parts, store, grant),
        None => Ok(()),
    }
}

pub(super) fn expire(parts: &Parts<'_>, store: &dyn Store, mut grant: Issued) -> Result<()> {
    grant.expires = Some(Instant::now());
    save(parts, store, &grant)
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

pub(super) fn save(parts: &Parts<'_>, store: &dyn Store, issued: &Issued) -> Result<()> {
    let raw = serde_json::to_string(&to_stored(issued))
        .map_err(|error| Error::Config(error.to_string()))?;
    store.set(parts.name, &raw)
}
