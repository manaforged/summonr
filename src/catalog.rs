use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Result;
use crate::prompt::summonr_home;

const FILE: &str = "models.json";

mod registry;

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
        match self.context {
            Some(tokens) if tokens >= 950_000 => {
                let millions = f64::from(tokens) / 1_000_000.0;
                if (millions - millions.round()).abs() < 0.1 {
                    format!("{}M", millions.round())
                } else {
                    format!("{millions:.1}M")
                }
            }
            Some(tokens) if tokens >= 1000 => format!("{}k", tokens / 1000),
            Some(tokens) => tokens.to_string(),
            None => String::new(),
        }
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
            .is_some_and(|day| now() / 86_400 <= day.saturating_add(14))
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

pub fn parse(value: &Value, fallback: Cost) -> Vec<Model> {
    value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| row(item, fallback))
        .collect()
}

fn row(item: &Value, fallback: Cost) -> Option<Model> {
    let raw = item
        .get("id")
        .or_else(|| item.get("name"))
        .and_then(Value::as_str)?;
    let id = raw.strip_prefix("models/").unwrap_or(raw);
    let parameters = item.get("supported_parameters").and_then(Value::as_array);
    let supports = |name: &str| parameters.map(|list| list.iter().any(|param| param == name));
    let capability = |name: &str| {
        item.get("capabilities")
            .and_then(|capabilities| capabilities.get(name))
            .and_then(Value::as_bool)
    };
    Some(Model {
        id: id.to_string(),
        display: item
            .get("display_name")
            .or_else(|| item.get("displayName"))
            .or_else(|| item.get("name"))
            .and_then(Value::as_str)
            .filter(|name| *name != raw)
            .map(str::to_string),
        context: window(item),
        about: item
            .get("description")
            .and_then(Value::as_str)
            .map(|text| text.split(". ").next().unwrap_or(text).trim().to_string())
            .filter(|text| !text.is_empty()),
        cost: priced(item).unwrap_or(fallback),
        release: item
            .get("created_at")
            .and_then(Value::as_str)
            .and_then(|date| registry::epoch_day(date.get(..10)?)),
        reasoning: supports("reasoning") == Some(true)
            || item.get("thinking").and_then(Value::as_bool) == Some(true),
        vision: capability("vision") == Some(true)
            || ["/architecture/input_modalities", "/input_modalities"]
                .into_iter()
                .filter_map(|pointer| item.pointer(pointer).and_then(Value::as_array))
                .any(|kinds| kinds.iter().any(|kind| kind == "image")),
        tools: supports("tools").or_else(|| capability("function_calling")),
        price: price(item),
        ..Model::default()
    })
}

pub(crate) fn window(item: &Value) -> Option<u32> {
    [
        "context_length",
        "context_window",
        "max_model_len",
        "max_context_length",
        "max_input_tokens",
        "inputTokenLimit",
    ]
    .into_iter()
    .filter_map(|field| item.get(field))
    .chain(item.pointer("/top_provider/context_length"))
    .find_map(tokens)
}

pub fn tokens(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .or_else(|| value.as_str()?.parse().ok())
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
}

fn price(item: &Value) -> Option<crate::provider::Price> {
    let pricing = item.get("pricing")?;
    let rate = |field: &str| {
        let value = pricing.get(field)?;
        value
            .as_f64()
            .or_else(|| value.as_str()?.parse().ok())
            .filter(|rate| *rate >= 0.0)
    };
    let (input, output, cached, scale) = if pricing.get("prompt").is_some() {
        ("prompt", "completion", "input_cache_read", 1_000_000.0)
    } else {
        ("input", "output", "cached_input", 1.0)
    };
    Some(crate::provider::Price {
        input: rate(input)? * scale,
        output: rate(output)? * scale,
        cached: rate(cached).unwrap_or_default() * scale,
    })
}

fn priced(item: &Value) -> Option<Cost> {
    let pricing = item.get("pricing")?;
    let charged = ["prompt", "completion", "input", "output"]
        .into_iter()
        .filter_map(|field| pricing.get(field))
        .any(|value| match value {
            Value::String(text) => text.parse::<f64>().is_ok_and(|rate| rate > 0.0),
            Value::Number(number) => number.as_f64().is_some_and(|rate| rate > 0.0),
            Value::Null | Value::Bool(_) | Value::Array(_) | Value::Object(_) => false,
        });
    Some(if charged { Cost::Metered } else { Cost::Free })
}

pub(crate) fn cached() -> BTreeMap<String, Listing> {
    load(FILE).unwrap_or_default()
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
    cached()
        .remove(endpoint)?
        .models
        .into_iter()
        .find(|model| model.id == id)
}

fn load<T: serde::de::DeserializeOwned + Default>(file: &str) -> Result<T> {
    match std::fs::read_to_string(summonr_home()?.join(file)) {
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|error| crate::error::Error::Config(format!("{file} is corrupt: {error}"))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn remember(name: &str, models: Vec<Model>) -> Result<()> {
    let path = summonr_home()?.join(FILE);
    let held = crate::durable::store::lock(&path, true)?;
    let mut all: BTreeMap<String, Listing> = load(FILE)?;
    all.insert(
        name.to_string(),
        Listing {
            models,
            fetched: now(),
        },
    );
    crate::durable::store::write(&path, serde_json::to_string(&all)?)?;
    drop(held);
    Ok(())
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
