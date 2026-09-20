use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::Result;
use crate::prompt::summonr_home;

const FILE: &str = "models.json";
const STALE: u64 = 6 * 3600;
const REGISTRY_URL: &str = "https://models.dev/api.json";
const REGISTRY_FILE: &str = "models-dev.json";
const REGISTRY_STALE: u64 = 24 * 3600;

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
    let mut models: Vec<Model> = value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| row(item, fallback))
        .collect();
    models.sort_by(|left, right| left.id.cmp(&right.id));
    models.dedup_by(|left, right| left.id == right.id);
    models
}

fn row(item: &Value, fallback: Cost) -> Option<Model> {
    let id = item
        .get("id")
        .or_else(|| item.get("name"))
        .and_then(Value::as_str)?;
    Some(Model {
        id: id.to_string(),
        display: item
            .get("display_name")
            .or_else(|| item.get("name"))
            .and_then(Value::as_str)
            .filter(|name| *name != id)
            .map(str::to_string),
        context: ["context_length", "context_window", "max_input_tokens"]
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

#[must_use]
pub fn plan_cost(name: &str) -> Cost {
    match crate::endpoint::auth_info(name) {
        Ok(info) if info.catalog_heading() == "Coding plans" => Cost::Plan,
        Ok(info) if info.catalog_heading() == "Local" => Cost::Free,
        Ok(_) | Err(_) => Cost::Unknown,
    }
}

pub fn cached() -> BTreeMap<String, Listing> {
    let Ok(home) = summonr_home() else {
        return BTreeMap::new();
    };
    std::fs::read_to_string(home.join(FILE))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn remember(name: &str, models: Vec<Model>) -> Result<()> {
    let mut all = cached();
    all.insert(
        name.to_string(),
        Listing {
            models,
            fetched: now(),
        },
    );
    let home = summonr_home()?;
    crate::durable::store::write(&home.join(FILE), serde_json::to_string(&all)?)?;
    Ok(())
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Meta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    display: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    context: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    metered: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct ProviderMeta {
    id: String,
    name: String,
    host: String,
    #[serde(default)]
    models: BTreeMap<String, Meta>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Registry {
    #[serde(default)]
    etag: String,
    #[serde(default)]
    fetched: u64,
    #[serde(default)]
    providers: Vec<ProviderMeta>,
}

fn registry() -> Registry {
    let Ok(home) = summonr_home() else {
        return Registry::default();
    };
    std::fs::read_to_string(home.join(REGISTRY_FILE))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn reduce(value: &Value) -> Registry {
    let providers = value
        .as_object()
        .into_iter()
        .flatten()
        .map(|(id, provider)| {
            let host = provider
                .get("api")
                .and_then(Value::as_str)
                .and_then(|api| url::Url::parse(api).ok())
                .and_then(|url| url.host_str().map(str::to_string))
                .unwrap_or_default();
            let models = provider
                .get("models")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .map(|(model_id, model)| {
                    let meta = Meta {
                        display: model
                            .get("name")
                            .and_then(Value::as_str)
                            .filter(|name| *name != *model_id)
                            .map(str::to_string),
                        context: model
                            .get("limit")
                            .and_then(|limit| limit.get("context"))
                            .and_then(Value::as_u64)
                            .and_then(|tokens| u32::try_from(tokens).ok()),
                        metered: model.get("cost").map(|cost| {
                            ["input", "output", "cache_read", "cache_write"]
                                .into_iter()
                                .find_map(|field| cost.get(field).and_then(Value::as_f64))
                                .is_some_and(|rate| rate > 0.0)
                        }),
                    };
                    (model_id.clone(), meta)
                })
                .collect();
            ProviderMeta {
                id: id.clone(),
                name: provider
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(id)
                    .to_string(),
                host,
                models,
            }
        })
        .collect();
    Registry {
        etag: String::new(),
        fetched: now(),
        providers,
    }
}

pub async fn refresh_registry() -> Result<()> {
    let mut cache = registry();
    if !cache.providers.is_empty() && now().saturating_sub(cache.fetched) < REGISTRY_STALE {
        return Ok(());
    }
    let http = crate::credentials::http()?;
    let url = std::env::var("SUMMONR_MODELS_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| REGISTRY_URL.to_string());
    let mut request = http.get(&url);
    if !cache.etag.is_empty() {
        request = request.header("if-none-match", cache.etag.as_str());
    }
    let mut response = request
        .send()
        .await
        .map_err(|error| crate::error::Error::Config(error.to_string()))?;
    if response.status().as_u16() == 304 {
        cache.fetched = now();
        let home = summonr_home()?;
        crate::durable::store::write(&home.join(REGISTRY_FILE), serde_json::to_string(&cache)?)?;
        return Ok(());
    }
    if !response.status().is_success() {
        return Err(crate::error::Error::Config(format!(
            "models.dev returned {}",
            response.status().as_u16()
        )));
    }
    let etag = response.header("etag").unwrap_or_default().to_string();
    let value: Value = response
        .json()
        .await
        .map_err(|error| crate::error::Error::Config(error.to_string()))?;
    let mut fresh = reduce(&value);
    fresh.etag = etag;
    let home = summonr_home()?;
    crate::durable::store::write(&home.join(REGISTRY_FILE), serde_json::to_string(&fresh)?)?;
    Ok(())
}

pub fn enrich(endpoint: &str, base_url: Option<&str>, models: &mut [Model]) {
    let registry = registry();
    if registry.providers.is_empty() {
        return;
    }
    let host = base_url
        .and_then(|base| url::Url::parse(base).ok())
        .and_then(|url| url.host_str().map(str::to_string));
    const ALIAS: &[(&str, &str)] = &[
        ("copilot", "github-copilot"),
        ("fireworks", "fireworks-ai"),
        ("gemini", "google"),
        ("glm", "zai"),
        ("glm-intl", "zai"),
        ("grok", "xai"),
        ("kimi", "moonshotai"),
        ("together", "togetherai"),
        ("zai-cn", "zai"),
    ];
    let alias = ALIAS
        .iter()
        .find_map(|(from, to)| (*from == endpoint).then_some(*to));
    let named = |provider: &&ProviderMeta| {
        [endpoint]
            .into_iter()
            .chain(alias)
            .any(|name| provider.id == name || provider.name.eq_ignore_ascii_case(name))
    };
    let provider = host
        .as_deref()
        .and_then(|host| {
            registry
                .providers
                .iter()
                .find(|provider| provider.host == host)
        })
        .or_else(|| registry.providers.iter().find(named));
    let Some(provider) = provider else {
        return;
    };
    for model in models.iter_mut() {
        let id = model.id.strip_prefix("models/").unwrap_or(&model.id);
        let Some(meta) = provider.models.get(id) else {
            continue;
        };
        if model.display.is_none() {
            model.display = meta.display.clone();
        }
        if model.context.is_none() {
            model.context = meta.context;
        }
        if model.cost == Cost::Unknown
            && let Some(metered) = meta.metered
        {
            model.cost = if metered { Cost::Metered } else { Cost::Free };
        }
    }
}
