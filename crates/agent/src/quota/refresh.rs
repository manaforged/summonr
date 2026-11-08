use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::Reading;

static VIEWED: Mutex<Option<Instant>> = Mutex::new(None);
static BATCH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// One row of the refresh table: the cadence while the last view is at most `viewed_within_secs` old.
#[derive(Deserialize, Clone, Copy, Debug)]
pub(super) struct Step {
    #[serde(default)]
    viewed_within_secs: Option<u64>,
    every_secs: u64,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    at: String,
    readings: Vec<Reading>,
}

/// The cadence for a last view `viewed` ago: the fixed `every_secs`, or the first matching row; `None` is manual.
pub(super) fn every(steps: &[Step], fixed: Option<u64>, viewed: Option<Duration>) -> Option<Duration> {
    if let Some(secs) = fixed {
        return (secs > 0).then(|| Duration::from_secs(secs));
    }
    steps
        .iter()
        .find(|step| match (step.viewed_within_secs, viewed) {
            (None, _) => true,
            (Some(within), Some(ago)) => ago.as_secs() <= within,
            (Some(_), None) => false,
        })
        .map(|step| Duration::from_secs(step.every_secs))
}

/// Notes a view now when `viewed`, and returns how long ago the last view was.
pub(super) fn mark(viewed: bool) -> Option<Duration> {
    let mut last = VIEWED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if viewed {
        *last = Some(Instant::now());
    }
    last.map(|at| at.elapsed())
}

/// The saved readings while they are younger than `every`; one batch runs at a time, and `None` reads again.
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
