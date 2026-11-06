use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// The paths a command changes in a folder outside Git, as the OS file watcher reports them.
pub(crate) struct Watch {
    root: PathBuf,
    own: Option<PathBuf>,
    events: Receiver<notify::Result<Event>>,
    _watcher: RecommendedWatcher,
}

impl Watch {
    /// Starts watching `root`; `None` inside Git, where `status_records` reports changes, or where
    /// the platform has no tree-wide watch that starts without walking the folder.
    pub(crate) fn start(root: &Path) -> Option<Self> {
        if !cfg!(any(target_os = "macos", windows)) || super::inside_git(root) {
            return None;
        }
        let root = dunce::canonicalize(root).ok()?;
        let (sender, events) = channel();
        let mut watcher = notify::recommended_watcher(sender).ok()?;
        watcher.watch(&root, RecursiveMode::Recursive).ok()?;
        let own = summonr_core::dirs::summonr_home().ok().and_then(|home| dunce::canonicalize(home).ok());
        Some(Self {
            root,
            own,
            events,
            _watcher: watcher,
        })
    }

    /// Waits for the watcher to go quiet, then returns the changed paths relative to the root.
    pub(crate) fn finish(self) -> Vec<String> {
        let limits = crate::limits::limits();
        let deadline = Instant::now() + limits.watch_settle();
        let mut paths = BTreeSet::new();
        while let Ok(event) = self.events.recv_timeout(limits.watch_quiet()) {
            if let Ok(event) = event
                && !matches!(event.kind, EventKind::Access(_))
            {
                paths.extend(event.paths.iter().filter_map(|path| self.relative(path)));
            }
            if Instant::now() >= deadline {
                break;
            }
        }
        paths.into_iter().collect()
    }

    fn relative(&self, path: &Path) -> Option<String> {
        if self.own.as_deref().is_some_and(|own| path.starts_with(own)) {
            return None;
        }
        let name = path.strip_prefix(&self.root).ok()?.to_string_lossy().replace('\\', "/");
        (!name.is_empty() && !super::ignored::hidden(&name)).then_some(name)
    }
}

#[cfg(all(test, any(target_os = "macos", windows)))]
#[path = "watch_tests.rs"]
mod tests;
