use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, UNIX_EPOCH};

use ignore::overrides::Override;
use ignore::{DirEntry, WalkBuilder, WalkState};

use crate::SKIP_DIRS;

use super::Query;

pub(super) fn collect(query: &Query) -> (Vec<(PathBuf, u64, u64)>, bool) {
    let root = query.root.as_path();
    if root.is_file() {
        let (modified, size) = std::fs::metadata(root).map_or((0, 0), |meta| stamp(&meta));
        return (vec![(root.to_path_buf(), modified, size)], false);
    }
    let mut walker = walker(root, query.types.clone(), query.no_ignore);
    walker.max_depth(query.depth);
    let found = Mutex::new(Vec::new());
    let stopped = AtomicBool::new(false);
    walker.build_parallel().run(|| {
        Box::new(|entry| {
            if Instant::now() >= query.deadline {
                stopped.store(true, Ordering::Relaxed);
                return WalkState::Quit;
            }
            if let Ok(entry) = entry
                && let Some(file) = kept(query, entry)
                && let Ok(mut files) = found.lock()
            {
                files.push(file);
            }
            WalkState::Continue
        })
    });
    (found.into_inner().unwrap_or_default(), stopped.into_inner())
}

fn kept(query: &Query, entry: DirEntry) -> Option<(PathBuf, u64, u64)> {
    if !entry.file_type().is_some_and(|kind| kind.is_file()) {
        return None;
    }
    let meta = entry.metadata();
    let path = entry.into_path();
    if !keep(
        &path,
        query.overrides.as_ref(),
        query.glob.is_some(),
        query.protect,
    ) {
        return None;
    }
    let stamp = match meta {
        Ok(meta) => stamp(&meta),
        Err(_) => path.metadata().map_or((0, 0), |meta| stamp(&meta)),
    };
    Some((path, stamp.0, stamp.1))
}

fn walker(root: &Path, types: Option<ignore::types::Types>, no_ignore: bool) -> WalkBuilder {
    let mut walker = WalkBuilder::new(root);
    walker
        .require_git(false)
        .hidden(false)
        .filter_entry(|entry| {
            entry.depth() == 0
                || !entry.file_type().is_some_and(|kind| kind.is_dir())
                || !SKIP_DIRS.contains(&entry.file_name().to_string_lossy().as_ref())
        });
    if no_ignore {
        walker
            .ignore(false)
            .git_ignore(false)
            .git_exclude(false)
            .parents(false);
    }
    if let Some(types) = types {
        walker.types(types);
    }
    walker
}

fn keep(path: &Path, overrides: Option<&Override>, require_match: bool, protect: bool) -> bool {
    if protect && crate::policy::protected_read(path) {
        return false;
    }
    let Some(overrides) = overrides else {
        return true;
    };
    let matched = overrides.matched(path, false);
    !(matched.is_ignore() || (require_match && !matched.is_whitelist()))
}

fn stamp(meta: &std::fs::Metadata) -> (u64, u64) {
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |span| span.as_secs());
    (modified, meta.len())
}
