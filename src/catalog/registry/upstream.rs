use std::collections::BTreeMap;

use serde::de::{DeserializeOwned, IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

use super::{Meta, ProviderMeta, REGISTRY_SCHEMA, Registry, epoch_day};
use crate::catalog::now;
use crate::error::Result;
use crate::provider::Price;

fn lenient<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    Ok(T::deserialize(Value::deserialize(deserializer)?).ok())
}

#[derive(Deserialize)]
struct Upstream {
    #[serde(default, deserialize_with = "lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    api: Option<String>,
    #[serde(default)]
    models: BTreeMap<String, UpstreamModel>,
}

#[derive(Deserialize)]
struct UpstreamModel {
    #[serde(default, deserialize_with = "lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    limit: Option<Limit>,
    #[serde(default, deserialize_with = "lenient")]
    cost: Option<Rates>,
    #[serde(default, deserialize_with = "lenient")]
    release_date: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    reasoning: Option<bool>,
    #[serde(default, deserialize_with = "lenient")]
    modalities: Option<Modalities>,
    #[serde(default, deserialize_with = "lenient")]
    family: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    status: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    tool_call: Option<bool>,
}

#[derive(Deserialize)]
struct Limit {
    #[serde(default, deserialize_with = "lenient")]
    context: Option<u64>,
}

#[derive(Deserialize)]
struct Rates {
    #[serde(default, deserialize_with = "lenient")]
    input: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    output: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    cache_read: Option<f64>,
    #[serde(default, deserialize_with = "lenient")]
    cache_write: Option<f64>,
}

#[derive(Deserialize)]
struct Modalities {
    #[serde(default, deserialize_with = "lenient")]
    input: Option<Vec<String>>,
}

const DEPRECATED: &str = "deprecated";
const IMAGE: &str = "image";

fn meta(model_id: &str, model: &UpstreamModel) -> Meta {
    let cost = model.cost.as_ref();
    Meta {
        display: model.name.clone().filter(|name| name != model_id),
        context: model
            .limit
            .as_ref()
            .and_then(|limit| limit.context)
            .and_then(|tokens| u32::try_from(tokens).ok()),
        metered: cost.map(|cost| {
            [cost.input, cost.output, cost.cache_read, cost.cache_write]
                .into_iter()
                .flatten()
                .any(|rate| rate > 0.0)
        }),
        release: model.release_date.as_deref().and_then(epoch_day),
        reasoning: model.reasoning == Some(true),
        vision: model
            .modalities
            .as_ref()
            .and_then(|modalities| modalities.input.as_ref())
            .is_some_and(|kinds| kinds.iter().any(|kind| kind == IMAGE)),
        family: model.family.clone(),
        deprecated: model.status.as_deref() == Some(DEPRECATED),
        tools: model.tool_call,
        price: cost.and_then(|cost| {
            let input = cost.input?;
            Some(Price {
                input,
                output: cost.output?,
                cached: cost.cache_read.unwrap_or(input),
                cache_write: cost.cache_write,
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

pub(in crate::catalog) fn decode(text: &str) -> Result<Registry> {
    let Reachable(upstream) = serde_json::from_str(text)?;
    Ok(reduce(upstream))
}

fn reduce(upstream: BTreeMap<String, Upstream>) -> Registry {
    let providers = upstream
        .into_iter()
        .map(|(id, provider)| ProviderMeta {
            name: provider.name.unwrap_or_else(|| id.clone()),
            api: provider
                .api
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
