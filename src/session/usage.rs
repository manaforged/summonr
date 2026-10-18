use std::collections::BTreeMap;
use std::path::Path;
use std::time::UNIX_EPOCH;

mod charges;

use self::charges::{Charge, Journals};
use super::catalog::list_in;
use crate::error::Result;
use crate::provider::Price;

pub(crate) const HOUR: u64 = 3600;
pub(crate) const DAY: u64 = 24 * HOUR;
const WEEK: u64 = 7 * DAY;
const FIXED: [(u64, usize, &str); 3] = [
    (HOUR, 24, "24h ago"),
    (6 * HOUR, 28, "7d ago"),
    (DAY, 30, "30d ago"),
];

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Total {
    pub(crate) input: u64,
    pub(crate) cached: u64,
    pub(crate) output: u64,
    pub(crate) cost: Option<f64>,
    pub(crate) turns: u64,
    pub(crate) millis: u64,
    pub(crate) sessions: u64,
}

impl Total {
    pub(crate) fn add(&mut self, row: &Spend) {
        self.input = self.input.saturating_add(row.input);
        self.cached = self.cached.saturating_add(row.cached);
        self.output = self.output.saturating_add(row.output);
        self.turns = self.turns.saturating_add(row.turns);
        self.millis = self.millis.saturating_add(row.millis);
        self.sessions = self.sessions.saturating_add(row.sessions);
        if let Some(cost) = row.cost {
            self.cost = Some(self.cost.unwrap_or(0.0) + cost);
        }
    }

    pub(crate) fn of(row: &Spend) -> Self {
        let mut total = Self::default();
        total.add(row);
        total
    }

    pub(crate) fn value(&self, cost: bool) -> f64 {
        if cost {
            self.cost.unwrap_or(0.0)
        } else {
            (self.input + self.output) as f64
        }
    }
}

pub(crate) fn buckets(
    range: usize,
    first: Option<u64>,
    now: u64,
    room: usize,
) -> (u64, usize, &'static str) {
    if let Some(fixed) = FIXED.get(range) {
        return *fixed;
    }
    let age = now.saturating_sub(first.unwrap_or(now)).max(DAY);
    let weeks = age.div_ceil(WEEK) as usize;
    if weeks <= room.max(1) {
        (WEEK, weeks.max(1), "first session")
    } else {
        (
            age.div_ceil(room as u64).div_ceil(DAY) * DAY,
            room,
            "first session",
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Spend {
    pub model: String,
    pub hour: u64,
    pub local: bool,
    pub workspace: String,
    pub input: u64,
    pub cached: u64,
    pub output: u64,
    pub cost: Option<f64>,
    pub turns: u64,
    pub millis: u64,
    pub sessions: u64,
}

pub(crate) fn spend(
    home: &Path,
    roots: &[String],
    prices: &BTreeMap<String, Price>,
) -> Result<Vec<Spend>> {
    let mut totals: BTreeMap<(String, u64, String), Spend> = BTreeMap::new();
    let sessions: Vec<_> = list_in(home)?
        .into_iter()
        .filter(|info| info.depth == 0)
        .collect();
    let mut journals = Journals::load(home);
    for info in &sessions {
        let Some(entry) = journals.entry(info) else {
            continue;
        };
        let local = info.cwd.as_ref().is_some_and(|cwd| roots.contains(cwd));
        let workspace = info.cwd.clone().unwrap_or_default();
        let fallback = if entry.undated {
            modified(&info.path)
        } else {
            0
        };
        let mut latest = None;
        for charge in &entry.charges {
            let model = charge
                .0
                .clone()
                .unwrap_or_else(|| info.model.clone().unwrap_or_default());
            let at = if charge.1 == 0 {
                fallback
            } else {
                charge.1 / 1000
            };
            let key = (model, at - at % HOUR, workspace.clone());
            latest = Some(key.clone());
            add(&mut totals, key, local, charge, prices);
        }
        if let Some(row) = latest.and_then(|key| totals.get_mut(&key)) {
            row.sessions += 1;
        }
    }
    journals.save(&sessions.iter().map(|info| info.id.as_str()).collect());
    Ok(totals.into_values().collect())
}

fn add(
    totals: &mut BTreeMap<(String, u64, String), Spend>,
    key: (String, u64, String),
    local: bool,
    charge: &Charge,
    prices: &BTreeMap<String, Price>,
) {
    let price = prices.get(&key.0);
    let row = totals
        .entry(key)
        .or_insert_with_key(|(model, hour, workspace)| Spend {
            cost: price.map(|_| 0.0),
            model: model.clone(),
            hour: *hour,
            local,
            workspace: workspace.clone(),
            ..Spend::default()
        });
    let usage = charge.usage();
    row.turns += 1;
    row.millis = row.millis.saturating_add(usage.millis);
    row.input = row.input.saturating_add(usage.input);
    row.cached = row.cached.saturating_add(usage.cached);
    row.output = row.output.saturating_add(usage.output);
    let cost = charge.cost().or_else(|| {
        price.map(|price| price.cost(usage.input, usage.output, usage.cached, usage.written))
    });
    if let Some(cost) = cost {
        *row.cost.get_or_insert(0.0) += cost;
    }
}

fn modified(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_secs())
}
