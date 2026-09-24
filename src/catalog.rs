use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Result;
use crate::prompt::summonr_home;

const FILE: &str = "models.json";
const STALE: u64 = 6 * 3600;

mod registry;

pub use registry::{enrich, refresh_registry};

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
    pub fn label(self) -> &'static str {
        match self {
            Self::Unknown => "",
            Self::Free => "free",
            Self::Plan => "plan",
            Self::Metered => "metered",
        }
    }

    #[must_use]
    pub fn rank(self) -> u8 {
        match self {
            Self::Free => 0,
            Self::Plan => 1,
            Self::Unknown => 2,
            Self::Metered => 3,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
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
}

impl Model {
    #[must_use]
    pub fn named(id: impl Into<String>) -> Self {
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
    pub fn window(&self) -> String {
        match self.context {
            Some(tokens) if tokens >= 1000 => format!("{}k", tokens / 1000),
            Some(tokens) => tokens.to_string(),
            None => String::new(),
        }
    }

    #[must_use]
    pub fn fresh(&self) -> bool {
        self.about.as_deref() == Some("new")
            || self
                .release
                .is_some_and(|day| now() / 86_400 <= day.saturating_add(14))
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Listing {
    #[serde(default)]
    pub models: Vec<Model>,
    #[serde(default)]
    pub fetched: u64,
}

impl Listing {
    #[must_use]
    pub fn stale(&self) -> bool {
        now().saturating_sub(self.fetched) > STALE
    }
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
    Some(Model {
        id: id.to_string(),
        display: item
            .get("display_name")
            .or_else(|| item.get("displayName"))
            .or_else(|| item.get("name"))
            .and_then(Value::as_str)
            .filter(|name| *name != raw)
            .map(str::to_string),
        context: [
            "context_length",
            "context_window",
            "max_input_tokens",
            "inputTokenLimit",
        ]
        .into_iter()
        .find_map(|field| item.get(field).and_then(Value::as_u64))
        .or_else(|| {
            item.get("top_provider")
                .and_then(|top| top.get("context_length"))
                .and_then(Value::as_u64)
        })
        .and_then(|tokens| u32::try_from(tokens).ok()),
        about: item
            .get("description")
            .and_then(Value::as_str)
            .map(|text| text.split(". ").next().unwrap_or(text).trim().to_string())
            .filter(|text| !text.is_empty()),
        cost: priced(item).unwrap_or(fallback),
        ..Model::default()
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

pub fn cached() -> BTreeMap<String, Listing> {
    load(FILE).unwrap_or_default()
}

fn load<T: serde::de::DeserializeOwned + Default>(file: &str) -> Result<T> {
    match std::fs::read_to_string(summonr_home()?.join(file)) {
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|error| crate::error::Error::Config(format!("{file} is corrupt: {error}"))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(error.into()),
    }
}

pub fn remember(name: &str, models: Vec<Model>) -> Result<()> {
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
