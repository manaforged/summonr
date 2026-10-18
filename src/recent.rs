use std::io;
use std::path::Path;

use crate::durable::store;
use crate::provider::Effort;

const FILE: &str = "recent.json";
const PINNED: &str = "favorites.json";
const CAP: usize = 8;

#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Seat {
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

fn rows(home: &Path, file: &str) -> Vec<(Seat, Option<Effort>)> {
    let Ok(text) = std::fs::read_to_string(home.join(file)) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(&text)
        .unwrap_or_default()
        .iter()
        .filter_map(|row| {
            let mut fields = row.splitn(3, '\t');
            let seat = Seat::new(fields.next()?, fields.next()?);
            Some((seat, fields.next().and_then(Effort::parse)))
        })
        .collect()
}

fn read(home: &Path, file: &str) -> Vec<Seat> {
    rows(home, file).into_iter().map(|(seat, _)| seat).collect()
}

fn update(
    home: &Path,
    file: &str,
    change: impl FnOnce(&mut Vec<(Seat, Option<Effort>)>),
) -> io::Result<()> {
    let path = home.join(file);
    let held = store::lock(&path, true)?;
    let mut seats = rows(home, file);
    change(&mut seats);
    let rows: Vec<String> = seats
        .iter()
        .map(|(seat, effort)| match effort {
            Some(effort) => format!("{}\t{}", seat.key(), effort.as_str()),
            None => seat.key(),
        })
        .collect();
    store::write(
        &path,
        serde_json::to_string(&rows)?,
        store::Durability::Renamed,
    )?;
    drop(held);
    Ok(())
}

pub(crate) fn load(home: &Path) -> Vec<Seat> {
    read(home, FILE)
}

pub(crate) fn pinned(home: &Path) -> Vec<Seat> {
    read(home, PINNED)
}

pub(crate) fn record(home: &Path, seat: &Seat, effort: Option<Effort>) -> io::Result<()> {
    update(home, FILE, |seats| {
        seats.retain(|(existing, _)| existing != seat);
        seats.insert(0, (seat.clone(), effort));
        seats.truncate(CAP);
    })
}

pub(crate) fn pin(home: &Path, seat: &Seat, on: bool) -> io::Result<()> {
    update(home, PINNED, |seats| {
        seats.retain(|(existing, _)| existing != seat);
        if on {
            seats.push((seat.clone(), None));
        }
    })
}

pub(crate) fn remembered_effort(
    home: Option<&Path>,
    endpoint: &str,
    model: &str,
) -> Option<Effort> {
    let seat = Seat::new(endpoint, model);
    rows(home?, FILE)
        .into_iter()
        .find(|(row, _)| *row == seat)
        .and_then(|(_, effort)| effort)
}

#[must_use]
pub fn default_effort(endpoint: &str, model: &str) -> Option<Effort> {
    let home = crate::prompt::summonr_home().ok();
    remembered_effort(home.as_deref(), endpoint, model).or_else(|| {
        crate::catalog::known(endpoint, model)
            .filter(|known| known.reasoning)
            .map(|_| Effort::High)
    })
}

#[cfg(test)]
#[path = "recent_tests.rs"]
mod tests;
