use std::path::{Path, PathBuf};

use crate::durable::store;

const FILE: &str = "recent.json";
const CAP: usize = 8;

fn path(home: &Path) -> PathBuf {
    home.join(FILE)
}

pub(crate) fn load(home: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path(home)) else {
        return Vec::new();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

pub(crate) fn record(home: &Path, seat: &str) {
    let mut seats = load(home);
    seats.retain(|existing| existing != seat);
    seats.insert(0, seat.to_string());
    seats.truncate(CAP);
    if let Ok(text) = serde_json::to_string(&seats) {
        drop(store::write(&path(home), text));
    }
}
