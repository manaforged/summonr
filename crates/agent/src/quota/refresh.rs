use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::Reading;

static BATCH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Serialize, Deserialize)]
struct Saved {
    at: String,
    readings: Vec<Reading>,
}

pub(super) async fn cached<F>(every: Option<Duration>, fetch: F) -> Vec<Reading>
where
    F: AsyncFnOnce() -> Vec<Reading>,
{
    let _batch = BATCH.lock().await;
    let saved = load();
    if let Some(saved) = &saved {
        let age =
            saved.at.parse::<jiff::Timestamp>().ok().and_then(|at| {
                u64::try_from(jiff::Timestamp::now().duration_since(at).as_secs()).ok()
            });
        let fresh = every.is_none_or(|every| age.is_some_and(|age| age < every.as_secs()));
        if fresh {
            return saved.readings.clone();
        }
    }
    let readings = keep_good(
        fetch().await,
        saved.map(|saved| saved.readings).unwrap_or_default(),
    );
    drop(store(&readings));
    readings
}

fn keep_good(fresh: Vec<Reading>, old: Vec<Reading>) -> Vec<Reading> {
    fresh
        .into_iter()
        .map(|mut reading| {
            if reading.error.is_some()
                && let Some(last) = old
                    .iter()
                    .find(|last| last.plan == reading.plan && !last.windows.is_empty())
            {
                reading.windows.clone_from(&last.windows);
                reading.at.clone_from(&last.at);
            }
            reading
        })
        .collect()
}

pub(super) fn folder() -> std::io::Result<PathBuf> {
    let home = summonr_core::dirs::summonr_home().map_err(std::io::Error::other)?;
    Ok(home.join("cache/quota"))
}

fn load() -> Option<Saved> {
    let path = folder().ok()?.join("plans.json");
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

pub(super) fn write_cache<T: Serialize + ?Sized>(name: &str, value: &T) -> std::io::Result<()> {
    let dir = folder()?;
    std::fs::create_dir_all(&dir)?;
    let bytes = serde_json::to_vec(value).map_err(std::io::Error::other)?;
    crate::durable::store::write(
        &dir.join(name),
        bytes,
        crate::durable::store::Durability::Renamed,
    )
}

fn store(readings: &[Reading]) -> std::io::Result<()> {
    let saved = Saved {
        at: jiff::Timestamp::now().to_string(),
        readings: readings.to_vec(),
    };
    write_cache("plans.json", &saved)
}
