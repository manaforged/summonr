use std::borrow::Cow;

use serde::Serialize;
use serde::ser::{SerializeMap, Serializer};
use serde_json::value::RawValue;
use serde_json::{Map, Value, json};

use crate::endpoint::CacheTtl;

#[derive(Serialize)]
#[serde(untagged)]
pub(super) enum Part<'a> {
    Value(Cow<'a, Value>),
    Use(Use<'a>),
}

impl<'a> Part<'a> {
    pub(super) fn owned(value: Value) -> Self {
        Self::Value(Cow::Owned(value))
    }

    pub(super) fn kind(&self) -> Option<&str> {
        match self {
            Self::Value(part) => part.get("type").and_then(Value::as_str),
            Self::Use(_) => Some("tool_use"),
        }
    }

    pub(super) fn thought(&self) -> bool {
        matches!(self.kind(), Some("thinking" | "redacted_thinking"))
    }

    pub(super) fn breakpoint(&mut self, ttl: CacheTtl) {
        match self {
            Self::Value(part) => part.to_mut()["cache_control"] = control(ttl),
            Self::Use(used) => used.cached = Some(ttl),
        }
    }
}

pub(super) fn control(ttl: CacheTtl) -> Value {
    match ttl {
        CacheTtl::FiveMinutes => json!({"type": "ephemeral"}),
        CacheTtl::OneHour => json!({"type": "ephemeral", "ttl": "1h"}),
    }
}

pub(super) enum Origin<'a> {
    Fresh(&'a str),
    Kept(&'a Map<String, Value>),
}

pub(super) struct Use<'a> {
    pub(super) origin: Origin<'a>,
    pub(super) name: &'a str,
    pub(super) input: &'a RawValue,
    pub(super) cached: Option<CacheTtl>,
}

enum Field<'a> {
    Value(&'a Value),
    Text(&'a str),
    Raw(&'a RawValue),
    Cache(CacheTtl),
}

impl Serialize for Field<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Value(value) => value.serialize(serializer),
            Self::Text(text) => text.serialize(serializer),
            Self::Raw(raw) => raw.serialize(serializer),
            Self::Cache(ttl) => control(*ttl).serialize(serializer),
        }
    }
}

impl Serialize for Use<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut fields: Vec<(&str, Field<'_>)> = match self.origin {
            Origin::Fresh(id) => vec![("id", Field::Text(id)), ("type", Field::Text("tool_use"))],
            Origin::Kept(block) => block
                .iter()
                .filter(|(key, _)| {
                    !matches!(key.as_str(), "name" | "input")
                        && !(self.cached.is_some() && key.as_str() == "cache_control")
                })
                .map(|(key, value)| (key.as_str(), Field::Value(value)))
                .collect(),
        };
        fields.push(("name", Field::Text(self.name)));
        fields.push(("input", Field::Raw(self.input)));
        if let Some(ttl) = self.cached {
            fields.push(("cache_control", Field::Cache(ttl)));
        }
        fields.sort_by_key(|(key, _)| *key);
        let mut map = serializer.serialize_map(Some(fields.len()))?;
        for (key, field) in &fields {
            map.serialize_entry(key, field)?;
        }
        map.end()
    }
}
