use std::collections::BTreeMap;
use std::path::Path;

use crate::session::{DAY, HOUR, Spend, Total, buckets};

const ROOM: usize = 78;
const PLACES: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Metric {
    Tokens,
    Cost,
}

pub(super) const METRICS: [Metric; 2] = [Metric::Tokens, Metric::Cost];

impl Metric {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Tokens => "tokens",
            Self::Cost => "cost",
        }
    }

    pub(super) const fn cost(self) -> bool {
        matches!(self, Self::Cost)
    }
}

pub(super) struct Report {
    pub(super) total: Total,
    pub(super) models: Vec<(String, Total)>,
    pub(super) columns: Vec<f64>,
    pub(super) series: Vec<Vec<f64>>,
    pub(super) span: u64,
    pub(super) first: &'static str,
    pub(super) places: Vec<(String, f64)>,
    pub(super) hours: [f64; 24],
}

impl Report {
    pub(super) fn build(rows: &[&Spend], range: usize, metric: Metric, now: u64) -> Self {
        let cost = metric.cost();
        let oldest = rows.iter().map(|row| row.hour).min();
        let (span, count, first) = buckets(range, oldest, now, ROOM);
        let start = now.saturating_sub(span * count as u64);
        let mut total = Total::default();
        let mut models: BTreeMap<&str, Total> = BTreeMap::new();
        let mut places: BTreeMap<String, f64> = BTreeMap::new();
        let mut hours = [0.0; 24];
        let mut columns = vec![0.0; count];
        let mut by_model: BTreeMap<&str, Vec<f64>> = BTreeMap::new();
        for row in rows.iter().filter(|row| row.hour + HOUR > start) {
            let value = Total::of(row).value(cost);
            total.add(row);
            models.entry(&row.model).or_default().add(row);
            *places.entry(place(&row.workspace)).or_default() += value;
            hours[((row.hour % DAY) / HOUR) as usize] += value;
            let at = (row.hour.saturating_sub(start) / span) as usize;
            let at = at.min(count - 1);
            columns[at] += value;
            by_model
                .entry(row.model.as_str())
                .or_insert_with(|| vec![0.0; count])[at] += value;
        }
        let mut models: Vec<(String, Total)> = models
            .into_iter()
            .map(|(name, total)| (name.to_string(), total))
            .collect();
        models.sort_by(|a, b| b.1.value(cost).total_cmp(&a.1.value(cost)));
        let series = models
            .iter()
            .map(|(name, _)| {
                by_model
                    .remove(name.as_str())
                    .unwrap_or_else(|| vec![0.0; count])
            })
            .collect();
        let mut places: Vec<(String, f64)> = places.into_iter().collect();
        places.sort_by(|a, b| b.1.total_cmp(&a.1));
        places.truncate(PLACES);
        Self {
            total,
            models,
            columns,
            series,
            span,
            first,
            places,
            hours,
        }
    }

    pub(super) fn cache_hit(&self) -> f64 {
        hit(&self.total)
    }
}

pub(super) fn hit(total: &Total) -> f64 {
    if total.input == 0 {
        0.0
    } else {
        total.cached as f64 * 100.0 / total.input as f64
    }
}

pub(super) fn turn_secs(total: &Total) -> f64 {
    if total.turns == 0 {
        0.0
    } else {
        total.millis as f64 / total.turns as f64 / 1000.0
    }
}

fn place(workspace: &str) -> String {
    Path::new(workspace).file_name().map_or_else(
        || "other".to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod tests;
