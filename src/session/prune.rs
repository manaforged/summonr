use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use crate::durable::log;

static STARTED: AtomicBool = AtomicBool::new(false);

pub fn prune(keep: Option<&str>) {
    if STARTED.swap(true, Ordering::Relaxed) {
        return;
    }
    let keep = keep.map(str::to_owned);
    std::thread::spawn(move || {
        let Ok(home) = crate::prompt::summonr_home() else {
            return;
        };
        let retain = crate::config::file()
            .map_or(crate::limits::limits().session_retain(), |file| {
                file.sessions.retain()
            });
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
    crate::durable::store::write(&stamp, b"", crate::durable::store::Durability::Renamed).is_ok()
}

pub(crate) fn prune_in(
    home: &Path,
    retain: Duration,
    keep: Option<&str>,
    now: SystemTime,
) -> usize {
    let Ok(entries) = std::fs::read_dir(log::sessions_dir(home)) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let stale = entry.file_type().is_ok_and(|kind| kind.is_dir())
            && keep != Some(name)
            && last_activity(&path)
                .is_some_and(|last| now.duration_since(last).is_ok_and(|age| age > retain));
        if stale && super::catalog::remove_in(home, name).is_ok() {
            removed += 1;
        }
    }
    removed
}

fn last_activity(dir: &Path) -> Option<SystemTime> {
    let own = std::fs::metadata(dir).and_then(|meta| meta.modified()).ok();
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter_map(|entry| entry.metadata().and_then(|meta| meta.modified()).ok())
        .chain(own)
        .max()
}
