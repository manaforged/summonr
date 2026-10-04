use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::prompt::summonr_home;

const FILE: &str = "models.json";

mod listing;
mod registry;
mod shelf;

pub(crate) use listing::window;
pub use listing::{parse, tokens};

pub(crate) use registry::enrich;
pub use registry::refresh_registry;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Cost {
    #[default]
    Unknown,
    Free,
    Plan,
    Metered,
}

impl Cost {
    #[must_use]
    pub(crate) fn rank(self) -> u8 {
        match self {
            Self::Free => 0,
            Self::Plan => 1,
            Self::Unknown => 2,
            Self::Metered => 3,
        }
    }
}

const CURRENT_DAYS: u64 = 183;
const FRESH_DAYS: u64 = 14;

#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
pub struct Model {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<String>,
    #[serde(default)]
    pub cost: Cost,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reasoning: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub vision: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub deprecated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<crate::provider::Price>,
}

impl Model {
    #[cfg(test)]
    pub(crate) fn named(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn label(&self) -> &str {
        self.display.as_deref().unwrap_or(&self.id)
    }

    #[must_use]
    pub(crate) fn window(&self) -> String {
        self.context.map_or_else(String::new, |tokens| {
            crate::provider::CountStyle::Window.format(u64::from(tokens))
        })
    }

    #[must_use]
    pub(crate) fn usable(&self) -> bool {
        !self.deprecated && self.tools != Some(false)
    }

    #[must_use]
    pub(crate) fn aged(&self) -> bool {
        self.release
            .is_some_and(|day| now() / 86_400 > day.saturating_add(CURRENT_DAYS))
    }

    #[must_use]
    pub(crate) fn released(&self) -> Option<String> {
        let (year, month, _) = civil(self.release?)?;
        Some(format!("{year}-{month:02}"))
    }

    #[must_use]
    pub(crate) fn released_on(&self) -> Option<String> {
        let (year, month, day) = civil(self.release?)?;
        Some(format!("{year}-{month:02}-{day:02}"))
    }

    #[must_use]
    pub(crate) fn fresh(&self) -> bool {
        self.release
            .is_some_and(|day| now() / 86_400 <= day.saturating_add(FRESH_DAYS))
    }
}

fn civil(epoch_day: u64) -> Option<(i64, i64, i64)> {
    let days = i64::try_from(epoch_day).ok()? + 719_468;
    let era = days.div_euclid(146_097);
    let of_era = days - era * 146_097;
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    Some((year, month, day))
}

#[must_use]
pub fn current(models: &[(&str, &Model)]) -> Vec<bool> {
    let mut newest: BTreeMap<(&str, &str), u64> = BTreeMap::new();
    for (endpoint, model) in models {
        if let (Some(family), Some(day)) = (model.family.as_deref(), model.release) {
            let held = newest.entry((endpoint, family)).or_insert(day);
            *held = (*held).max(day);
        }
    }
    models
        .iter()
        .map(|(endpoint, model)| {
            !model.aged()
                && match (model.family.as_deref(), model.release) {
                    (Some(family), Some(day)) => newest.get(&(*endpoint, family)) == Some(&day),
                    (None, _) | (Some(_), None) => true,
                }
        })
        .collect()
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct Listing {
    #[serde(default)]
    pub models: Vec<Model>,
    #[serde(default)]
    pub fetched: u64,
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or_default()
}

pub(crate) fn cached() -> BTreeMap<String, Listing> {
    (*listings()).clone()
}

fn listings() -> Arc<BTreeMap<String, Listing>> {
    shelf::load(FILE).unwrap_or_default()
}

#[must_use]
pub fn accepts_images(endpoint: &crate::Endpoint) -> bool {
    endpoint.images.unwrap_or_else(|| {
        known(&endpoint.name, &endpoint.model).map_or_else(
            || vision_of(endpoint, &registry::current()),
            |model| model.vision,
        )
    })
}

fn vision_of(endpoint: &crate::Endpoint, registry: &registry::Registry) -> bool {
    let mut models = [Model {
        id: endpoint.model.clone(),
        ..Model::default()
    }];
    registry::enrich_from(
        registry,
        &endpoint.name,
        endpoint.base_url.as_deref(),
        &mut models,
    );
    models[0].vision
}

#[must_use]
pub(crate) fn known(endpoint: &str, id: &str) -> Option<Model> {
    listings()
        .get(endpoint)?
        .models
        .iter()
        .find(|model| model.id == id)
        .cloned()
}

pub(crate) fn remember(name: &str, models: Vec<Model>) -> Result<()> {
    let path = summonr_home()?.join(FILE);
    let held = crate::durable::store::lock(&path, true)?;
    let mut all: BTreeMap<String, Listing> =
        (*shelf::load::<BTreeMap<String, Listing>>(FILE)?).clone();
    all.insert(
        name.to_string(),
        Listing {
            models,
            fetched: now(),
        },
    );
    crate::durable::store::write(&path, serde_json::to_string(&all)?)?;
    shelf::store(&path, all);
    drop(held);
    Ok(())
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
