use serde_json::Value;

use super::{Cost, Model, registry};

const LISTED: &str = "list";

pub fn parse(value: &Value, fallback: Cost) -> Vec<Model> {
    let rows: Vec<&Value> = value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    let curated = rows.iter().any(|item| visibility(item) == Some(LISTED));
    rows.into_iter()
        .filter(|item| !curated || visibility(item) == Some(LISTED))
        .filter_map(|item| row(item, fallback))
        .collect()
}

fn visibility(item: &Value) -> Option<&str> {
    item.get("visibility").and_then(Value::as_str)
}

fn row(item: &Value, fallback: Cost) -> Option<Model> {
    let raw = item
        .get("id")
        .or_else(|| item.get("slug"))
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
    let (input, output, cached, written, scale) = if pricing.get("prompt").is_some() {
        (
            "prompt",
            "completion",
            "input_cache_read",
            "input_cache_write",
            1_000_000.0,
        )
    } else {
        ("input", "output", "cached_input", "cache_write", 1.0)
    };
    let input = rate(input)?;
    Some(crate::provider::Price {
        input: input * scale,
        output: rate(output)? * scale,
        cached: rate(cached).unwrap_or(input) * scale,
        cache_write: rate(written).map(|rate| rate * scale),
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
