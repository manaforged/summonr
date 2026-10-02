use std::collections::BTreeMap;

use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use super::{Cost, Model, load, now};
use crate::error::Result;
use crate::prompt::summonr_home;
use crate::provider::Price;

const REGISTRY_URL: &str = "https://models.dev/api.json";
pub(super) const FILE: &str = "models-dev.json";
const REGISTRY_STALE: u64 = 3600;
const REGISTRY_SCHEMA: u32 = 3;
const SNAPSHOT: &str = include_str!("models-dev.json");

pub(super) fn epoch_day(date: &str) -> Option<u64> {
    let mut parts = date.splitn(3, '-').map(str::parse::<i64>);
    let (year, month, day) = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year - era * 400;
    let of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let of_cycle = of_era * 365 + of_era / 4 - of_era / 100 + of_year;
    u64::try_from(era * 146_097 + of_cycle - 719_468).ok()
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct Meta {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    display: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    context: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    metered: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    release: Option<u64>,
    #[serde(default)]
    reasoning: bool,
    #[serde(default)]
    vision: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    family: Option<String>,
    #[serde(default)]
    deprecated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tools: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    price: Option<Price>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
struct ProviderMeta {
    id: String,
    name: String,
    #[serde(default)]
    api: String,
    #[serde(default)]
    models: BTreeMap<String, Meta>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(super) struct Registry {
    #[serde(default)]
    schema: u32,
    #[serde(default)]
    etag: String,
    #[serde(default)]
    fetched: u64,
    #[serde(default)]
    providers: Vec<ProviderMeta>,
}

#[derive(Deserialize)]
struct Upstream {
    #[serde(default)]
    name: Option<Value>,
    #[serde(default)]
    api: Option<Value>,
    #[serde(default)]
    models: BTreeMap<String, UpstreamModel>,
}

#[derive(Deserialize)]
struct UpstreamModel {
    #[serde(default)]
    name: Option<Value>,
    #[serde(default)]
    limit: Option<Value>,
    #[serde(default)]
    cost: Option<Value>,
    #[serde(default)]
    release_date: Option<Value>,
    #[serde(default)]
    reasoning: Option<Value>,
    #[serde(default)]
    modalities: Option<Value>,
    #[serde(default)]
    family: Option<Value>,
    #[serde(default)]
    status: Option<Value>,
    #[serde(default)]
    tool_call: Option<Value>,
}

fn meta(model_id: &str, model: &UpstreamModel) -> Meta {
    let text = |value: &Option<Value>| value.as_ref().and_then(Value::as_str).map(str::to_string);
    let cost = model.cost.as_ref();
    Meta {
        display: text(&model.name).filter(|name| name != model_id),
        context: model
            .limit
            .as_ref()
            .and_then(|limit| limit.get("context"))
            .and_then(Value::as_u64)
            .and_then(|tokens| u32::try_from(tokens).ok()),
        metered: cost.map(|cost| {
            ["input", "output", "cache_read", "cache_write"]
                .into_iter()
                .filter_map(|field| cost.get(field).and_then(Value::as_f64))
                .any(|rate| rate > 0.0)
        }),
        release: model
            .release_date
            .as_ref()
            .and_then(Value::as_str)
            .and_then(epoch_day),
        reasoning: model.reasoning.as_ref().and_then(Value::as_bool) == Some(true),
        vision: model
            .modalities
            .as_ref()
            .and_then(|modalities| modalities.get("input"))
            .and_then(Value::as_array)
            .is_some_and(|kinds| kinds.iter().any(|kind| kind == "image")),
        family: text(&model.family),
        deprecated: model.status.as_ref().and_then(Value::as_str) == Some("deprecated"),
        tools: model.tool_call.as_ref().and_then(Value::as_bool),
        price: cost.and_then(|cost| {
            let rate = |field: &str| cost.get(field).and_then(Value::as_f64);
            Some(Price {
                input: rate("input")?,
                output: rate("output")?,
                cached: rate("cache_read").unwrap_or_default(),
            })
        }),
    }
}

struct Reachable(BTreeMap<String, Upstream>);

impl<'de> Deserialize<'de> for Reachable {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct Keep;
        impl<'de> Visitor<'de> for Keep {
            type Value = BTreeMap<String, Upstream>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a models.dev provider map")
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut kept = BTreeMap::new();
                while let Some(id) = map.next_key::<String>()? {
                    if crate::endpoint::catalog_providers().any(|known| known == id) {
                        kept.insert(id, map.next_value::<Upstream>()?);
                    } else {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(kept)
            }
        }
        deserializer.deserialize_map(Keep).map(Reachable)
    }
}

pub(super) fn decode(text: &str) -> Result<Registry> {
    let Reachable(upstream) = serde_json::from_str(text)?;
    Ok(reduce(upstream))
}

pub(super) fn snapshot() -> Registry {
    serde_json::from_str(SNAPSHOT).unwrap_or_default()
}

pub(super) fn current() -> Registry {
    load::<Registry>(FILE)
        .ok()
        .filter(|cache| cache.schema == REGISTRY_SCHEMA && !cache.providers.is_empty())
        .unwrap_or_else(snapshot)
}

fn reduce(upstream: BTreeMap<String, Upstream>) -> Registry {
    let providers = upstream
        .into_iter()
        .map(|(id, provider)| ProviderMeta {
            name: provider
                .name
                .as_ref()
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_string(),
            api: provider
                .api
                .as_ref()
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim_end_matches('/')
                .to_string(),
            models: provider
                .models
                .iter()
                .map(|(model_id, model)| (model_id.clone(), meta(model_id, model)))
                .collect(),
            id,
        })
        .collect();
    Registry {
        schema: REGISTRY_SCHEMA,
        etag: String::new(),
        fetched: now(),
        providers,
    }
}

pub async fn refresh_registry() -> Result<()> {
    let cache = load::<Registry>(FILE)
        .ok()
        .filter(|cache| cache.schema == REGISTRY_SCHEMA)
        .unwrap_or_default();
    if now().saturating_sub(cache.fetched) < REGISTRY_STALE {
        return Ok(());
    }
    let saved = fetch_registry(&cache.etag).await?.unwrap_or(Registry {
        fetched: now(),
        ..cache
    });
    let home = summonr_home()?;
    crate::durable::store::write(&home.join(FILE), serde_json::to_string(&saved)?)?;
    Ok(())
}

async fn fetch_registry(etag: &str) -> Result<Option<Registry>> {
    let http = crate::credentials::http()?;
    let url = std::env::var("SUMMONR_MODELS_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(|| REGISTRY_URL.to_string());
    let mut request = http.get(&url);
    if !etag.is_empty() {
        request = request.header("if-none-match", etag);
    }
    let response = request
        .send()
        .await
        .map_err(|error| crate::error::Error::Config(error.to_string()))?;
    match response.status().as_u16() {
        304 => return Ok(None),
        200..=299 => {}
        code => {
            return Err(crate::error::Error::Config(format!(
                "models.dev returned {code}"
            )));
        }
    }
    let etag = response.header("etag").unwrap_or_default().to_string();
    let text = response
        .text()
        .await
        .map_err(|error| crate::error::Error::Config(error.to_string()))?;
    Ok(Some(Registry {
        etag,
        ..decode(&text)?
    }))
}

fn authority(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    Some(format!(
        "{}:{}",
        url.host_str()?,
        url.port_or_known_default()?
    ))
}

pub(crate) fn enrich(endpoint: &str, base_url: Option<&str>, models: &mut [Model]) {
    enrich_from(&current(), endpoint, base_url, models);
}

pub(super) fn enrich_from(
    registry: &Registry,
    endpoint: &str,
    base_url: Option<&str>,
    models: &mut [Model],
) {
    if registry.providers.is_empty() {
        return;
    }
    let base = base_url.unwrap_or_default().trim_end_matches('/');
    let named = |provider: &&ProviderMeta| {
        [endpoint]
            .into_iter()
            .chain(crate::endpoint::provider_ids(endpoint).iter().copied())
            .any(|name| provider.id == name || provider.name.eq_ignore_ascii_case(name))
    };
    let providers = &registry.providers;
    let wanted = authority(base);
    let same_host: Vec<&ProviderMeta> = providers
        .iter()
        .filter(|provider| wanted.is_some() && authority(&provider.api) == wanted)
        .collect();
    let provider = providers
        .iter()
        .find(|provider| !base.is_empty() && provider.api == base)
        .or_else(|| same_host.first().copied())
        .or_else(|| providers.iter().find(named));
    for model in models.iter_mut() {
        let own = provider.and_then(|provider| provider.models.get(&model.id));
        let Some(meta) = own
            .or_else(|| {
                same_host
                    .iter()
                    .find_map(|provider| provider.models.get(&model.id))
            })
            .or_else(|| {
                providers
                    .iter()
                    .find_map(|provider| provider.models.get(&model.id))
            })
        else {
            continue;
        };
        apply(model, meta);
        if let Some(own) = own {
            price(model, own);
        }
    }
}

fn apply(model: &mut Model, meta: &Meta) {
    if model.display.is_none() {
        model.display = meta.display.clone();
    }
    if model.context.is_none() {
        model.context = meta.context;
    }
    model.release = model.release.or(meta.release);
    model.reasoning |= meta.reasoning;
    model.vision |= meta.vision;
    model.family = model.family.take().or_else(|| meta.family.clone());
    model.deprecated |= meta.deprecated;
    model.tools = model.tools.or(meta.tools);
}

fn price(model: &mut Model, meta: &Meta) {
    model.price = model.price.or(meta.price);
    if model.cost == Cost::Unknown
        && let Some(metered) = meta.metered
    {
        model.cost = if metered { Cost::Metered } else { Cost::Free };
    }
}
