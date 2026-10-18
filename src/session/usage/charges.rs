use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::durable::log::{self, Usage};
use crate::session::SessionInfo;
use crate::session::sidecar::Sidecar;

const CACHE_FILE: &str = "usage.json";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Charge(
    pub(super) Option<String>,
    pub(super) u64,
    pub(super) u64,
    pub(super) u64,
    pub(super) u64,
    pub(super) u64,
    pub(super) u64,
    pub(super) Option<u64>,
);

impl Charge {
    fn new(model: Option<String>, at: u64, usage: Usage, cost: Option<f64>) -> Self {
        Self(
            model,
            at,
            usage.input,
            usage.cached,
            usage.output,
            usage.written,
            usage.millis,
            cost.map(f64::to_bits),
        )
    }

    pub(super) fn usage(&self) -> Usage {
        Usage {
            input: self.2,
            cached: self.3,
            output: self.4,
            written: self.5,
            millis: self.6,
            ..Usage::default()
        }
    }

    pub(super) fn cost(&self) -> Option<f64> {
        self.7.map(f64::from_bits)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(super) struct Entry {
    size: u64,
    pub(super) undated: bool,
    pub(super) charges: Vec<Charge>,
}

#[derive(Deserialize)]
struct Record {
    #[serde(default)]
    at: u64,
    #[serde(default)]
    reported: Option<bool>,
    #[serde(flatten)]
    line: log::Line,
}

pub(super) struct Journals(Sidecar<Entry>);

impl Journals {
    pub(super) fn load(home: &Path) -> Self {
        Self(Sidecar::load(home, CACHE_FILE))
    }

    pub(super) fn entry(&mut self, info: &SessionInfo) -> Option<&Entry> {
        let size = std::fs::metadata(&info.path).ok()?.len();
        let fresh = self.0.get(&info.id).is_some_and(|entry| entry.size == size);
        if !fresh {
            let Some((undated, charges)) = read(&info.path) else {
                self.0.forget(&info.id);
                return None;
            };
            self.0.put(
                &info.id,
                Entry {
                    size,
                    undated,
                    charges,
                },
            );
        }
        self.0.get(&info.id)
    }

    pub(super) fn save(self, live: &BTreeSet<&str>) {
        self.0.save(live.iter().copied());
    }
}

fn read(path: &Path) -> Option<(bool, Vec<Charge>)> {
    let records = log::records::<Record>(path).ok()?;
    let mut current = None;
    let mut charges = Vec::new();
    let mut undated = false;
    for record in records {
        let reported = record.reported;
        let (model, usage, cost) = match record.line {
            log::Line::Session { model, .. } => {
                current = Some(model);
                continue;
            }
            log::Line::Assistant {
                model, usage, cost, ..
            } => (model, usage, cost),
            log::Line::Charge {
                model, usage, cost, ..
            } if reported != Some(false) => (model, usage, cost),
            _ => continue,
        };
        if usage.input == 0 && usage.output == 0 {
            continue;
        }
        let model = if model.is_empty() {
            current.clone()
        } else {
            Some(model)
        };
        undated |= record.at == 0;
        charges.push(Charge::new(model, record.at, usage, cost));
    }
    Some((undated, charges))
}
