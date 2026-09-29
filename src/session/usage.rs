use std::collections::BTreeMap;
use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::Deserialize;

use super::catalog::list_in;
use crate::durable::log::{self, Usage};
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

#[derive(Default)]
pub(crate) struct Total {
    pub(crate) input: u64,
    pub(crate) cached: u64,
    pub(crate) output: u64,
    pub(crate) cost: Option<f64>,
}

impl Total {
    pub(crate) fn add(&mut self, row: &Spend) {
        self.input = self.input.saturating_add(row.input);
        self.cached = self.cached.saturating_add(row.cached);
        self.output = self.output.saturating_add(row.output);
        if let Some(cost) = row.cost {
            self.cost = Some(self.cost.unwrap_or(0.0) + cost);
        }
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
    pub input: u64,
    pub cached: u64,
    pub output: u64,
    pub cost: Option<f64>,
}

#[derive(Deserialize)]
struct Record {
    #[serde(default)]
    at: u64,
    kind: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    usage: Option<Usage>,
}

pub(crate) fn spend(
    home: &Path,
    roots: &[String],
    prices: &BTreeMap<String, Price>,
) -> Result<Vec<Spend>> {
    let mut totals: BTreeMap<(String, u64, bool), Spend> = BTreeMap::new();
    for info in list_in(home)?.into_iter().filter(|info| info.depth == 0) {
        let Ok(records) = log::records::<Record>(&info.path) else {
            continue;
        };
        let local = info.cwd.as_ref().is_some_and(|cwd| roots.contains(cwd));
        let fallback = modified(&info.path);
        let mut current = info.model.clone().unwrap_or_default();
        for record in records {
            if record.kind == "session" {
                current = record.model;
                continue;
            }
            let Some(usage) = record.usage else {
                continue;
            };
            if usage.input == 0 && usage.output == 0 {
                continue;
            }
            let model = if record.model.is_empty() {
                current.clone()
            } else {
                record.model
            };
            let at = if record.at == 0 {
                fallback
            } else {
                record.at / 1000
            };
            let hour = at - at % HOUR;
            let row = totals
                .entry((model.clone(), hour, local))
                .or_insert_with(|| Spend {
                    cost: prices.get(&model).map(|_| 0.0),
                    model: model.clone(),
                    hour,
                    local,
                    ..Spend::default()
                });
            row.input = row.input.saturating_add(usage.input);
            row.cached = row.cached.saturating_add(usage.cached);
            row.output = row.output.saturating_add(usage.output);
            if let (Some(cost), Some(price)) = (row.cost.as_mut(), prices.get(&model)) {
                *cost += price.cost(usage.input, usage.output, usage.cached);
            }
        }
    }
    Ok(totals.into_values().collect())
}

fn modified(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_secs())
}
