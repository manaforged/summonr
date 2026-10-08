use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerKind {
    Mcp,
    Lsp,
}

pub(crate) type Slot<T> = Arc<tokio::sync::Mutex<Option<T>>>;

#[derive(Clone, Debug)]
pub(crate) struct Live {
    pub(crate) kind: ServerKind,
    pub(crate) name: String,
    pub(crate) rss_kb: Option<u64>,
}

trait Held: Send + Sync {
    fn release(&self, still_idle: &dyn Fn() -> bool) -> Release;
}

enum Release {
    Stopped,
    Busy,
    Fresh,
    Empty,
}

impl<T: Send> Held for tokio::sync::Mutex<Option<T>> {
    fn release(&self, still_idle: &dyn Fn() -> bool) -> Release {
        let Ok(mut slot) = self.try_lock() else {
            return Release::Busy;
        };
        if slot.is_none() {
            return Release::Empty;
        }
        if !still_idle() {
            return Release::Fresh;
        }
        drop(slot.take());
        Release::Stopped
    }
}

struct Entry {
    id: usize,
    kind: ServerKind,
    name: String,
    pid: Option<u32>,
    idle: Option<Duration>,
    used: Instant,
    held: Weak<dyn Held>,
}

static BOARD: Mutex<Vec<Entry>> = Mutex::new(Vec::new());

fn id<T>(slot: &Slot<T>) -> usize {
    Arc::as_ptr(slot).cast::<()>().addr()
}

pub(crate) fn register<T: Send + 'static>(
    kind: ServerKind,
    name: &str,
    pid: Option<u32>,
    idle: Option<Duration>,
    slot: &Slot<T>,
) {
    let id = id(slot);
    let held: Arc<dyn Held> = slot.clone();
    let entry = Entry {
        id,
        kind,
        name: name.to_string(),
        pid,
        idle,
        used: Instant::now(),
        held: Arc::downgrade(&held),
    };
    let (fresh, excess, crowd) = {
        let Ok(mut board) = BOARD.lock() else {
            return;
        };
        board.retain(|entry| entry.held.strong_count() > 0);
        let fresh = match board.iter_mut().find(|entry| entry.id == id) {
            Some(found) => {
                *found = entry;
                false
            }
            None => {
                board.push(entry);
                true
            }
        };
        let excess = board
            .len()
            .saturating_sub(crate::limits::limits().servers_max);
        let mut crowd: Vec<&Entry> = board
            .iter()
            .filter(|entry| entry.id != id && entry.idle.is_some())
            .collect();
        crowd.sort_by_key(|entry| entry.used);
        let crowd: Vec<(usize, Weak<dyn Held>)> = crowd
            .into_iter()
            .map(|entry| (entry.id, entry.held.clone()))
            .collect();
        (fresh, excess, crowd)
    };
    evict(excess, crowd);
    if fresh
        && let Some(idle) = idle
        && let Ok(runtime) = tokio::runtime::Handle::try_current()
    {
        drop(runtime.spawn(reap(id, idle)));
    }
}

pub(crate) fn touch<T>(slot: &Slot<T>) {
    touch_id(id(slot));
}

pub(crate) fn live() -> Vec<Live> {
    let found: Vec<(ServerKind, String, Option<u32>)> = BOARD.lock().map_or_else(
        |_poison| Vec::new(),
        |board| {
            board
                .iter()
                .filter(|entry| entry.held.strong_count() > 0)
                .map(|entry| (entry.kind, entry.name.clone(), entry.pid))
                .collect()
        },
    );
    found
        .into_iter()
        .map(|(kind, name, pid)| Live {
            kind,
            name,
            rss_kb: pid.and_then(|pid| footprint_kb(Some(pid))),
        })
        .collect()
}

fn evict(mut excess: usize, crowd: Vec<(usize, Weak<dyn Held>)>) {
    for (id, held) in crowd {
        if excess == 0 {
            return;
        }
        let Some(held) = held.upgrade() else {
            forget(id);
            excess -= 1;
            continue;
        };
        if let Release::Stopped | Release::Empty = held.release(&|| true) {
            forget(id);
            excess -= 1;
        }
    }
}

fn forget(id: usize) {
    if let Ok(mut board) = BOARD.lock() {
        board.retain(|entry| entry.id != id);
    }
}

fn idle_for(id: usize) -> Option<Duration> {
    BOARD
        .lock()
        .ok()?
        .iter()
        .find(|entry| entry.id == id)
        .map(|entry| entry.used.elapsed())
}

async fn reap(id: usize, idle: Duration) {
    loop {
        let Some(wait) = idle_for(id).map(|spent| idle.saturating_sub(spent)) else {
            return;
        };
        tokio::time::sleep(wait).await;
        let Some(held) = BOARD.lock().ok().and_then(|board| {
            board
                .iter()
                .find(|entry| entry.id == id)
                .and_then(|entry| entry.held.upgrade())
        }) else {
            forget(id);
            return;
        };
        match held.release(&|| idle_for(id).is_some_and(|spent| spent >= idle)) {
            Release::Stopped | Release::Empty => {
                forget(id);
                return;
            }
            Release::Busy => touch_id(id),
            Release::Fresh => {}
        }
    }
}

fn touch_id(id: usize) {
    if let Ok(mut board) = BOARD.lock()
        && let Some(entry) = board.iter_mut().find(|entry| entry.id == id)
    {
        entry.used = Instant::now();
    }
}

#[cfg(target_os = "macos")]
#[expect(unsafe_code, reason = "proc_pid_rusage has no std wrapper")]
pub(crate) fn footprint_kb(pid: Option<u32>) -> Option<u64> {
    let target = match pid {
        Some(pid) => i32::try_from(pid).ok()?,
        None => i32::try_from(std::process::id()).ok()?,
    };
    let mut info = std::mem::MaybeUninit::<libc::rusage_info_v4>::zeroed();
    // SAFETY: proc_pid_rusage writes one rusage_info_v4 into the buffer; zero means success.
    let info = unsafe {
        (libc::proc_pid_rusage(target, libc::RUSAGE_INFO_V4, info.as_mut_ptr().cast()) == 0)
            .then(|| info.assume_init())
    }?;
    Some(
        match pid {
            Some(_) => info.ri_phys_footprint,
            None => info.ri_lifetime_max_phys_footprint,
        } / 1024,
    )
}

#[cfg(all(unix, not(target_os = "macos")))]
#[expect(unsafe_code, reason = "getrusage has no std wrapper")]
pub(crate) fn footprint_kb(pid: Option<u32>) -> Option<u64> {
    if let Some(pid) = pid {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        return status
            .lines()
            .find_map(|line| line.strip_prefix("VmRSS:"))?
            .trim()
            .trim_end_matches("kB")
            .trim()
            .parse()
            .ok();
    }
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: getrusage writes one rusage into the zeroed buffer; zero means success.
    let usage = unsafe {
        (libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) == 0).then(|| usage.assume_init())
    }?;
    u64::try_from(usage.ru_maxrss).ok()
}

#[cfg(not(unix))]
pub(crate) fn footprint_kb(_pid: Option<u32>) -> Option<u64> {
    None
}
