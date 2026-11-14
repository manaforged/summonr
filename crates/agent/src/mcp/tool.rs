use serde_json::{Value, json};

use super::config::Server;

#[derive(Clone)]
pub(super) struct Tool {
    pub(super) alias: String,
    pub(super) name: String,
    pub(super) description: String,
    pub(super) schema: Value,
}

impl Tool {
    pub(super) fn parse(server: &str, item: &Value) -> Option<Self> {
        let name = item.get("name")?.as_str()?.to_string();
        let alias = summonr_core::text::clean_name(&format!("{server}__{name}"), "_-", '_', 64);
        Some(Self {
            alias,
            description: item
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            schema: item
                .get("inputSchema")
                .cloned()
                .unwrap_or_else(|| json!({"type": "object"})),
            name,
        })
    }
}

pub(super) fn offered<'a>(
    server: &'a Server,
    items: &'a [Value],
) -> impl Iterator<Item = (&'a Value, Tool)> + 'a {
    items
        .iter()
        .filter_map(|item| Tool::parse(&server.name, item).map(|tool| (item, tool)))
        .filter(|(_, tool)| {
            server
                .allow
                .as_ref()
                // complexity: bounded by a BTreeSet lookup, O(log n)
                .is_none_or(|allow| allow.contains(&tool.name))
        })
}

#[cfg(test)]
#[path = "tool_tests.rs"]
mod tests;
