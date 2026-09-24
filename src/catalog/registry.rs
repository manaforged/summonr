use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Cost, Model, load, now};
use crate::error::Result;
use crate::prompt::summonr_home;

const REGISTRY_URL: &str = "https://models.dev/api.json";
const REGISTRY_FILE: &str = "models-dev.json";
const REGISTRY_STALE: u64 = 3600;

fn epoch_day(date: &str) -> Option<u64> {
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
struct Registry {
    #[serde(default)]
    etag: String,
    #[serde(default)]
    fetched: u64,
    #[serde(default)]
    providers: Vec<ProviderMeta>,
}

fn reduce(value: &Value) -> Registry {
    let providers = value
        .as_object()
        .into_iter()
        .flatten()
        .map(|(id, provider)| {
            let api = provider
                .get("api")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim_end_matches('/');
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
                                .filter_map(|field| cost.get(field).and_then(Value::as_f64))
                                .any(|rate| rate > 0.0)
                        }),
                        release: model
                            .get("release_date")
                            .and_then(Value::as_str)
                            .and_then(epoch_day),
                        reasoning: model.get("reasoning").and_then(Value::as_bool) == Some(true),
                        vision: model
                            .pointer("/modalities/input")
                            .and_then(Value::as_array)
                            .is_some_and(|kinds| kinds.iter().any(|kind| kind == "image")),
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
                api: api.to_string(),
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
    let cache: Registry = load(REGISTRY_FILE).unwrap_or_default();
    if now().saturating_sub(cache.fetched) < REGISTRY_STALE {
        return Ok(());
    }
    let saved = fetch_registry(&cache.etag).await?.unwrap_or(Registry {
        fetched: now(),
        ..cache
    });
    let home = summonr_home()?;
    crate::durable::store::write(&home.join(REGISTRY_FILE), serde_json::to_string(&saved)?)?;
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
    let mut response = request
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
    let value: Value = response
        .json()
        .await
        .map_err(|error| crate::error::Error::Config(error.to_string()))?;
    Ok(Some(Registry {
        etag,
        ..reduce(&value)
    }))
}

pub fn enrich(endpoint: &str, base_url: Option<&str>, models: &mut [Model]) {
    let registry: Registry = load(REGISTRY_FILE).unwrap_or_default();
    if registry.providers.is_empty() {
        return;
    }
    let base = base_url.unwrap_or_default().trim_end_matches('/');
    let host = |url: &str| Some(url::Url::parse(url).ok()?.host_str()?.to_string());
    let named = |provider: &&ProviderMeta| {
        [endpoint]
            .into_iter()
            .chain(crate::endpoint::provider_ids(endpoint).iter().copied())
            .any(|name| provider.id == name || provider.name.eq_ignore_ascii_case(name))
    };
    let providers = &registry.providers;
    let provider = providers
        .iter()
        .find(|provider| !base.is_empty() && provider.api == base)
        .or_else(|| {
            let wanted = host(base)?;
            providers
                .iter()
                .find(|provider| host(&provider.api).as_deref() == Some(wanted.as_str()))
        })
        .or_else(|| providers.iter().find(named));
    let Some(provider) = provider else {
        return;
    };
    for model in models.iter_mut() {
        let Some(meta) = provider.models.get(&model.id) else {
            model.about.get_or_insert_with(|| "new".to_string());
            continue;
        };
        if model.display.is_none() {
            model.display = meta.display.clone();
        }
        if model.context.is_none() {
            model.context = meta.context;
        }
        model.release = model.release.or(meta.release);
        model.reasoning |= meta.reasoning;
        model.vision |= meta.vision;
        if model.cost == Cost::Unknown
            && let Some(metered) = meta.metered
        {
            model.cost = if metered { Cost::Metered } else { Cost::Free };
        }
    }
}
