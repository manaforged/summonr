use std::io;
use std::path::Path;

use crate::durable::store;

const FILE: &str = "recent.json";
const PINNED: &str = "favorites.json";
const CAP: usize = 8;

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Seat {
    pub endpoint: String,
    pub model: String,
}

impl Seat {
    pub(crate) fn new(endpoint: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            model: model.into(),
        }
    }

    fn key(&self) -> String {
        format!("{}\t{}", self.endpoint, self.model)
    }
}

fn read(home: &Path, file: &str) -> Vec<Seat> {
    let Ok(text) = std::fs::read_to_string(home.join(file)) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(&text)
        .unwrap_or_default()
        .iter()
        .filter_map(|row| row.split_once('\t'))
        .map(|(endpoint, model)| Seat::new(endpoint, model))
        .collect()
}

fn update(home: &Path, file: &str, change: impl FnOnce(&mut Vec<Seat>)) -> io::Result<()> {
    let path = home.join(file);
    let held = store::lock(&path, true)?;
    let mut seats = read(home, file);
    change(&mut seats);
    let rows: Vec<String> = seats.iter().map(Seat::key).collect();
    store::write(&path, serde_json::to_string(&rows)?)?;
    drop(held);
    Ok(())
}

pub(crate) fn load(home: &Path) -> Vec<Seat> {
    read(home, FILE)
}

pub(crate) fn pinned(home: &Path) -> Vec<Seat> {
    read(home, PINNED)
}

pub(crate) fn record(home: &Path, seat: &Seat) -> io::Result<()> {
    update(home, FILE, |seats| {
        seats.retain(|existing| existing != seat);
        seats.insert(0, seat.clone());
        seats.truncate(CAP);
    })
}

pub(crate) fn pin(home: &Path, seat: &Seat, on: bool) -> io::Result<()> {
    update(home, PINNED, |seats| {
        seats.retain(|existing| existing != seat);
        if on {
            seats.push(seat.clone());
        }
    })
}
