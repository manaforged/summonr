use std::collections::BTreeMap;
use std::sync::{Arc, OnceLock};

use leyline::http::StatusCode;
use serde::{Deserialize, Serialize};

use super::{Cost, Model, now, shelf};
use crate::error::Result;
use crate::prompt::summonr_home;
use crate::provider::Price;

const REGISTRY_URL: &str = "https://models.dev/api.json";
pub(super) const FILE: &str = "models-dev.json";
const REGISTRY_SCHEMA: u32 = 3;
const SNAPSHOT: &str = include_str!("models-dev.json");

mod upstream;

pub(super) use upstream::decode;

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

pub(super) fn snapshot() -> Arc<Registry> {
    static PARSED: OnceLock<Arc<Registry>> = OnceLock::new();
    Arc::clone(PARSED.get_or_init(|| {
        Arc::new(
            serde_json::from_str(SNAPSHOT)
                .expect("bundled provider registry snapshot must parse as a Registry"),
        )
    }))
}

#[cfg(test)]
pub(crate) fn serves(providers: &[&str], model: &str) -> Option<bool> {
    serves_in(&snapshot(), providers, model)
}

#[cfg(test)]
pub(crate) async fn serves_live() -> Result<impl Fn(&[&str], &str) -> Option<bool>> {
    let registry = fetch_registry("")
        .await?
        .ok_or_else(|| crate::error::Error::Config("models.dev sent no catalog".to_string()))?;
    Ok(move |providers: &[&str], model: &str| serves_in(&registry, providers, model))
}

#[cfg(test)]
fn serves_in(registry: &Registry, providers: &[&str], model: &str) -> Option<bool> {
    let by_id: BTreeMap<&str, &ProviderMeta> = registry
        .providers
        .iter()
        .map(|provider| (provider.id.as_str(), provider))
        .collect();
    let known: Vec<&ProviderMeta> = providers
        .iter()
        .filter_map(|id| by_id.get(id).copied())
        .collect();
    if known.is_empty() {
        return None;
    }
    Some(known.iter().any(|provider| {
        provider
            .models
            .get(model)
            .is_some_and(|meta| !meta.deprecated)
    }))
}

pub(super) fn current() -> Arc<Registry> {
    shelf::load::<Registry>(FILE)
        .ok()
        .filter(|cache| cache.schema == REGISTRY_SCHEMA && !cache.providers.is_empty())
        .unwrap_or_else(snapshot)
}

pub async fn refresh_registry() -> Result<()> {
    if crate::config::file().is_ok_and(|file| file.models_catalog == Some(false)) {
        return Ok(());
    }
    let cache = shelf::load::<Registry>(FILE)
        .ok()
        .filter(|cache| cache.schema == REGISTRY_SCHEMA)
        .map_or_else(Registry::default, |cache| (*cache).clone());
    if now().saturating_sub(cache.fetched) < crate::limits::limits().models_catalog_secs {
        return Ok(());
    }
    let saved = fetch_registry(&cache.etag).await?.unwrap_or(Registry {
        fetched: now(),
        ..cache
    });
    let path = summonr_home()?.join(FILE);
    crate::durable::store::write(&path, serde_json::to_string(&saved)?)?;
    shelf::store(&path, saved);
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
    let status = response.status();
    if status == StatusCode::NOT_MODIFIED {
        return Ok(None);
    }
    if !status.is_success() {
        return Err(crate::error::Error::Config(format!(
            "models.dev returned {}",
            status.as_u16()
        )));
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

pub(crate) fn offered(endpoint: &str, cost: Cost) -> Vec<Model> {
    let registry = current();
    let by_id: BTreeMap<&str, &ProviderMeta> = registry
        .providers
        .iter()
        .map(|provider| (provider.id.as_str(), provider))
        .collect();
    crate::endpoint::provider_ids(endpoint)
        .iter()
        .filter_map(|id| by_id.get(id))
        .flat_map(|provider| provider.models.iter())
        .filter(|(_, meta)| !meta.deprecated)
        .map(|(id, meta)| {
            let mut model = Model::named(id.clone());
            model.apply(meta);
            model.cost = cost;
            model
        })
        .collect()
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
        model.apply(meta);
        if let Some(own) = own {
            model.price(own);
        }
    }
}

impl Model {
    fn apply(&mut self, meta: &Meta) {
        if self.display.is_none() {
            self.display = meta.display.clone();
        }
        if self.context.is_none() {
            self.context = meta.context;
        }
        self.release = self.release.or(meta.release);
        self.reasoning |= meta.reasoning;
        self.vision |= meta.vision;
        self.family = self.family.take().or_else(|| meta.family.clone());
        self.deprecated |= meta.deprecated;
        self.tools = self.tools.or(meta.tools);
    }

    fn price(&mut self, meta: &Meta) {
        self.price = self.price.or(meta.price);
        if self.cost == Cost::Unknown
            && let Some(metered) = meta.metered
        {
            self.cost = if metered { Cost::Metered } else { Cost::Free };
        }
    }
}
