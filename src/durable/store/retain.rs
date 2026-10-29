use std::path::Path;
use std::time::{Duration, SystemTime};

pub(crate) fn retention() -> Duration {
    crate::config::file().map_or(crate::limits::limits().session_retain(), |file| {
        file.sessions.retain()
    })
}

pub(crate) fn retain(
    dir: &Path,
    age: Duration,
    now: SystemTime,
    mut remove: impl FnMut(&Path) -> bool,
) -> usize {
    let mut removed = 0;
    for entry in summonr_core::walk::children(dir) {
        let path = entry.path();
        let stale = last_activity(&path)
            .is_some_and(|last| now.duration_since(last).is_ok_and(|idle| idle > age));
        if stale && remove(&path) {
            removed += 1;
        }
    }
    removed
}

fn last_activity(path: &Path) -> Option<SystemTime> {
    let own = std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok();
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.metadata().and_then(|meta| meta.modified()).ok())
        .chain(own)
        .max()
}
