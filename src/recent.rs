//! Most-recently-used model seats, for the picker's recent section.
//!
//! One small file of state the picker reads so it opens on the seats the
//! user actually uses, newest first. Written beside the config in the mage
//! home. A missing or unreadable file is an empty history.

use std::path::{Path, PathBuf};

const FILE: &str = "recent.json";
const CAP: usize = 8;

fn path(home: &Path) -> PathBuf {
    home.join(FILE)
}

/// The seats used, newest first, as `endpoint\tmodel` rows. Empty when the
/// file is missing or unreadable.
pub(crate) fn load(home: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path(home)) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Move `seat` to the front, cap the length, and write it back. A write
/// error is dropped: recency is a convenience, not state a turn depends on.
pub(crate) fn record(home: &Path, seat: &str) {
    let mut seats = load(home);
    seats.retain(|existing| existing != seat);
    seats.insert(0, seat.to_string());
    seats.truncate(CAP);
    if let Ok(text) = serde_json::to_string(&seats) {
        drop(std::fs::write(path(home), text));
    }
}

#[cfg(test)]
#[path = "recent_tests.rs"]
mod tests;
