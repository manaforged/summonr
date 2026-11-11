use std::path::{Path, PathBuf};

use super::*;

/// Removes the inbox and lock file of a session that is gone, once older than the sweep window; the audit keeps every message.
pub(crate) fn sweep_inboxes(dir: &Path, now: std::time::SystemTime) -> usize {
    let window = std::time::Duration::from_secs(crate::limits::limits().mail_sweep_secs);
    let live: std::collections::HashSet<String> = super::live(dir)
        .into_iter()
        .map(|agent| clean_id(&agent.id))
        .collect();
    let entries: Vec<PathBuf> = summonr_core::walk::children(&dir.join("inbox"))
        .map(|entry| entry.path())
        .collect();
    let inboxes = entries
        .iter()
        .filter(|path| stale_inbox(path.as_path(), &live, window, now))
        .filter(|path| std::fs::remove_file(path.as_path()).is_ok())
        .count();
    let locks = entries
        .iter()
        .filter(|path| stale_lock(path.as_path(), &live, window, now))
        .filter(|path| std::fs::remove_file(path.as_path()).is_ok())
        .count();
    inboxes + locks
}

fn older_than(path: &Path, window: std::time::Duration, now: std::time::SystemTime) -> bool {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .is_ok_and(|at| now.duration_since(at).is_ok_and(|age| age > window))
}

fn stale_inbox(
    path: &Path,
    live: &std::collections::HashSet<String>,
    window: std::time::Duration,
    now: std::time::SystemTime,
) -> bool {
    let Some(id) = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(".jsonl"))
    else {
        return false;
    };
    older_than(path, window, now) && !live.contains(id)
}

fn stale_lock(
    path: &Path,
    live: &std::collections::HashSet<String>,
    window: std::time::Duration,
    now: std::time::SystemTime,
) -> bool {
    let Some(inbox) = path
        .file_name()
        .and_then(|name| name.to_str()?.strip_suffix(".lock"))
    else {
        return false;
    };
    let Some(id) = inbox.strip_suffix(".jsonl") else {
        return false;
    };
    let empty = std::fs::metadata(path.with_file_name(inbox)).map_or(true, |meta| meta.len() == 0);
    older_than(path, window, now) && empty && !live.contains(id)
}
