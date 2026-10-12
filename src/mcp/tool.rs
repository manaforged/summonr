use serde_json::{Value, json};

pub(super) struct Tool {
    pub(super) alias: String,
    pub(super) name: String,
    pub(super) description: String,
    pub(super) schema: Value,
}

impl Tool {
    pub(super) fn parse(server: &str, item: &Value) -> Option<Self> {
        let name = item.get("name")?.as_str()?.to_string();
        let alias: String = format!("{server}__{name}")
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '_' | '-') {
                    c
                } else {
                    '_'
                }
            })
            .take(64)
            .collect();
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

#[cfg(test)]
#[path = "tool_tests.rs"]
mod tests;
