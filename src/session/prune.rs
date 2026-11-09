use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use crate::durable::{log, store};

static STARTED: AtomicBool = AtomicBool::new(false);

pub fn prune(keep: Option<&str>) {
    if STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let keep = keep.map(str::to_owned);
    std::thread::spawn(move || {
        let Ok(home) = summonr_core::dirs::summonr_home() else {
            return;
        };
        let retain = store::retention();
        let period = Duration::from_secs(crate::limits::limits().session_prune_secs);
        if due(&home, period, SystemTime::now()) {
            prune_in(&home, retain, keep.as_deref(), SystemTime::now());
        }
    });
}

const STAMP_FILE: &str = "pruned";

pub(crate) fn due(home: &Path, period: Duration, now: SystemTime) -> bool {
    let stamp = log::sessions_dir(home).join(STAMP_FILE);
    let recent = std::fs::metadata(&stamp)
        .and_then(|meta| meta.modified())
        .is_ok_and(|at| now.duration_since(at).is_ok_and(|age| age < period));
    if recent || !log::sessions_dir(home).is_dir() {
        return false;
    }
    store::write(&stamp, b"", store::Durability::Renamed).is_ok()
}

pub(crate) fn prune_in(
    home: &Path,
    retain: Duration,
    keep: Option<&str>,
    now: SystemTime,
) -> usize {
    store::retain(&log::sessions_dir(home), retain, now, |path| {
        let name = path.file_name().and_then(|name| name.to_str());
        path.is_dir()
            && name.is_some_and(|name| {
                keep != Some(name) && super::catalog::remove_in(home, name).is_ok()
            })
    })
}

#[cfg(test)]
#[path = "prune_tests.rs"]
mod tests;
