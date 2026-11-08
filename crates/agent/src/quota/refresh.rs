use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::Reading;

static BATCH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Serialize, Deserialize)]
struct Saved {
    at: String,
    readings: Vec<Reading>,
}

/// The saved readings while younger than `every` (always, when `None`), else a new read; one batch runs at a time.
pub(super) async fn cached<F>(every: Option<Duration>, fetch: F) -> Vec<Reading>
where
    F: AsyncFnOnce() -> Vec<Reading>,
{
    let _batch = BATCH.lock().await;
    let saved = load();
    if let Some(saved) = &saved {
        let age = saved
            .at
            .parse::<jiff::Timestamp>()
            .ok()
            .and_then(|at| u64::try_from(jiff::Timestamp::now().duration_since(at).as_secs()).ok());
        let fresh = every.is_none_or(|every| age.is_some_and(|age| age < every.as_secs()));
        if fresh {
            return saved.readings.clone();
        }
    }
    let readings = keep_good(fetch().await, saved.map(|saved| saved.readings).unwrap_or_default());
    drop(store(&readings));
    readings
}

/// Fresh readings, where a plan that failed keeps its last windows and their time.
fn keep_good(fresh: Vec<Reading>, old: Vec<Reading>) -> Vec<Reading> {
    fresh
        .into_iter()
        .map(|mut reading| {
            if reading.error.is_some()
                && let Some(last) = old.iter().find(|last| last.plan == reading.plan && !last.windows.is_empty())
            {
                reading.windows.clone_from(&last.windows);
                reading.at.clone_from(&last.at);
            }
            reading
        })
        .collect()
}

fn file() -> Option<std::path::PathBuf> {
    summonr_core::dirs::summonr_home().ok().map(|home| home.join("cache/quota/plans.json"))
}

fn load() -> Option<Saved> {
    serde_json::from_slice(&std::fs::read(file()?).ok()?).ok()
}

fn store(readings: &[Reading]) -> std::io::Result<()> {
    let path = file().ok_or_else(|| std::io::Error::other("no summonr home"))?;
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let saved = Saved {
        at: jiff::Timestamp::now().to_string(),
        readings: readings.to_vec(),
    };
    let bytes = serde_json::to_vec(&saved).map_err(std::io::Error::other)?;
    crate::durable::store::write(&path, bytes, crate::durable::store::Durability::Renamed)
}
