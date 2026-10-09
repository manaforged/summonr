use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::{Instant, UNIX_EPOCH};

use grep_searcher::Searcher;
use ignore::overrides::Override;
use ignore::{WalkBuilder, WalkState};

use crate::error::{Error, Result};
use crate::{SKIP_DIRS, STATE_DIRS};

use super::scan::{self, Found};
use super::{Query, Root, filter};

#[derive(Default)]
pub(super) struct Report {
    pub(super) found: Vec<Found>,
    pub(super) files: usize,
    pub(super) match_total: usize,
    pub(super) searched: usize,
    pub(super) skipped: usize,
    pub(super) capped: usize,
    pub(super) unreadable: usize,
    pub(super) links: usize,
    pub(super) stopped: bool,
    pub(super) groups: BTreeMap<String, usize>,
}

#[derive(Default)]
pub(super) struct Shared {
    report: Mutex<Report>,
    stop: AtomicBool,
    pub(super) searched: AtomicUsize,
    pub(super) skipped: AtomicUsize,
    unreadable: AtomicUsize,
    links: AtomicUsize,
    seen: Mutex<HashSet<PathBuf>>,
}

impl Shared {
    fn add(&self, found: Found, keep: usize, group: Option<String>) {
        let mut report = self.report.lock().unwrap_or_else(PoisonError::into_inner);
        report.files += 1;
        report.match_total += found.hits.matches;
        report.capped += usize::from(found.hits.over);
        if let Some(group) = group {
            *report.groups.entry(group).or_default() += 1;
        }
        report.found.push(found);
        if report.found.len() >= keep.saturating_mul(2) {
            newest_first(&mut report.found);
            report.found.truncate(keep);
        }
    }

    fn first_visit(&self, path: &Path) -> bool {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(path.to_path_buf())
    }

    fn halt(&self) {
        self.stop.store(true, Ordering::Relaxed);
        self.report
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stopped = true;
    }

    fn take(&self) -> Report {
        let mut report =
            std::mem::take(&mut *self.report.lock().unwrap_or_else(PoisonError::into_inner));
        report.searched = self.searched.load(Ordering::Relaxed);
        report.skipped = self.skipped.load(Ordering::Relaxed);
        report.unreadable = self.unreadable.load(Ordering::Relaxed);
        report.links = self.links.load(Ordering::Relaxed);
        newest_first(&mut report.found);
        report
    }
}

pub(super) fn run(query: &Arc<Query>) -> Result<Report> {
    let shared = Arc::new(Shared::default());
    let (done, finished) = mpsc::channel::<()>();
    let (owned, state) = (Arc::clone(query), Arc::clone(&shared));
    let worker = std::thread::Builder::new()
        .name("search".to_string())
        .spawn(move || {
            walk_all(&owned, &state);
            drop(done);
        })
        .map_err(|error| Error::Tool(format!("search could not start: {error}")))?;
    let wait = query.deadline.saturating_duration_since(Instant::now());
    match finished.recv_timeout(wait) {
        Err(mpsc::RecvTimeoutError::Timeout) => shared.halt(),
        Err(mpsc::RecvTimeoutError::Disconnected) | Ok(()) => {
            if worker.join().is_err() {
                return Err(Error::Tool(
                    "search failed while it walked the files".to_string(),
                ));
            }
        }
    }
    Ok(shared.take())
}

const SKIP_ROOTS: &[&str] = &["/System/Volumes/Data", "/proc", "/sys", "/dev"];

fn walk_all(query: &Query, shared: &Shared) {
    let (files, dirs): (Vec<&Root>, Vec<&Root>) =
        query.roots.iter().partition(|root| root.path.is_file());
    let mut searcher = scan::searcher(query);
    for root in files {
        visit_path(query, shared, root, root.path.clone(), &mut searcher);
    }
    let Some((first, rest)) = dirs.split_first() else {
        return;
    };
    let mut walker = walker(&first.path, query.types.clone(), query.no_ignore);
    for root in rest {
        walker.add(&root.path);
    }
    walker.max_depth(query.depth);
    walker.build_parallel().run(|| {
        let mut searcher = scan::searcher(query);
        let dirs = &dirs;
        Box::new(move |entry| {
            if shared.stop.load(Ordering::Relaxed) {
                return WalkState::Quit;
            }
            if Instant::now() >= query.deadline {
                shared.halt();
                return WalkState::Quit;
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    shared.unreadable.fetch_add(1, Ordering::Relaxed);
                    return WalkState::Continue;
                }
            };
            if entry.file_type().is_some_and(|kind| {
                kind.is_file()
                    || (query.matcher.is_none() && kind.is_symlink() && !entry.path().is_dir())
            }) && let Some(root) = dirs
                .iter()
                .find(|root| entry.path().starts_with(&root.path))
            {
                visit_path(query, shared, root, entry.into_path(), &mut searcher);
            }
            WalkState::Continue
        })
    });
}

fn visit_path(query: &Query, shared: &Shared, root: &Root, path: PathBuf, searcher: &mut Searcher) {
    if query.matcher.is_none() {
        shared.searched.fetch_add(1, Ordering::Relaxed);
    }
    if !keep(&path, root.overrides.as_ref(), query.glob)
        || !filter::named(&query.name, &path)
        || (query.roots.len() > 1 && !shared.first_visit(&path))
    {
        return;
    }
    let (modified, size) = path
        .metadata()
        .or_else(|_| path.symlink_metadata())
        .map_or((0, 0), |meta| stamp(&meta));
    if query.newer.is_some_and(|newer| modified < newer) {
        return;
    }
    if query.matcher.is_none()
        && path
            .symlink_metadata()
            .is_ok_and(|meta| meta.file_type().is_symlink())
    {
        shared.links.fetch_add(1, Ordering::Relaxed);
    }
    let found = match &query.matcher {
        Some(matcher) => scan::file(query, shared, searcher, matcher, &path, size),
        None => Some(scan::Hits::default()),
    };
    if let Some(hits) = found {
        let group = query.group.then(|| filter::folder(&path));
        shared.add(
            Found {
                path,
                modified,
                hits,
            },
            query.offset.saturating_add(query.limit),
            group,
        );
    }
}

fn newest_first(found: &mut [Found]) {
    found.sort_by(|left, right| {
        right
            .modified
            .cmp(&left.modified)
            .then_with(|| left.path.cmp(&right.path))
    });
}

fn walker(root: &Path, types: Option<ignore::types::Types>, no_ignore: bool) -> WalkBuilder {
    let mut walker = WalkBuilder::new(root);
    let skipped: &'static [&'static str] = if no_ignore { STATE_DIRS } else { SKIP_DIRS };
    walker
        .require_git(false)
        .hidden(false)
        .filter_entry(move |entry| {
            entry.depth() == 0
                || !entry.file_type().is_some_and(|kind| kind.is_dir())
                || !(skipped.contains(&entry.file_name().to_string_lossy().as_ref())
                    || SKIP_ROOTS
                        .iter()
                        .any(|mirror| entry.path() == Path::new(mirror))
                    || crate::policy::protected_read(entry.path()))
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

fn keep(path: &Path, overrides: Option<&Override>, require_match: bool) -> bool {
    if crate::policy::protected_read(path) {
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
