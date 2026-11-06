use std::io;
use std::path::{Path, PathBuf};
use std::thread::{self, JoinHandle};

use super::super::log;
use super::{Snapshot, cache_dir, remembered, snapshot};
use crate::durable::driver::canonical;

/// A capture running off the turn path.
pub(crate) type Pending = JoinHandle<io::Result<Snapshot>>;

/// The exclusion list for a session's captures: its store and the tree cache.
pub(crate) fn excludes(session: &Path) -> Vec<PathBuf> {
    let store = session.parent().and_then(Path::parent).map(canonical);
    store.into_iter().chain(cache_path(session)).collect()
}

/// The tree cache directory beside a session's home.
pub(crate) fn cache_path(session: &Path) -> Option<PathBuf> {
    log::home_of(session).map(|home| cache_dir(&canonical(home)))
}

/// Spawns the Execute base capture off the turn path.
pub(crate) fn begin(workspace: PathBuf, session: &Path, previous: Option<Snapshot>) -> Pending {
    let excludes = excludes(session);
    let cache = cache_path(session);
    thread::spawn(move || {
        let previous = previous.or_else(|| {
            cache
                .as_deref()
                .and_then(|cache| remembered(cache, &workspace))
        });
        snapshot(&workspace, &excludes, previous.as_ref(), &[])
    })
}

/// Waits for a pending capture, letting its panic resume.
pub(crate) fn settle(task: Pending) -> io::Result<Snapshot> {
    task.join()
        .unwrap_or_else(|payload| std::panic::resume_unwind(payload))
}
