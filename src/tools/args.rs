use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::error::{Error, Result};
use crate::provider::{FunctionDefinition, ToolDefinition, ToolKind};

pub(crate) fn tool(name: &str, description: &str, parameters: Value) -> ToolDefinition {
    ToolDefinition {
        kind: ToolKind::Function,
        function: FunctionDefinition {
            name: name.to_string(),
            description: description.to_string(),
            parameters: object_root(parameters),
        },
    }
}

fn object_root(parameters: Value) -> Value {
    let Value::Object(mut root) = parameters else {
        return serde_json::json!({"type": "object"});
    };
    let mut names = required(&Value::Object(root.clone()));
    for key in ["anyOf", "oneOf", "allOf"] {
        let Some(Value::Array(branches)) = root.remove(key) else {
            continue;
        };
        let mut shared: Option<BTreeSet<String>> = None;
        for branch in &branches {
            let wanted = required(branch);
            shared = Some(match (key, shared) {
                ("allOf", Some(all)) => all.union(&wanted).cloned().collect(),
                (_, Some(all)) => all.intersection(&wanted).cloned().collect(),
                (_, None) => wanted,
            });
            let Some(Value::Object(fields)) = branch.get("properties") else {
                continue;
            };
            if let Value::Object(properties) = root
                .entry("properties")
                .or_insert_with(|| Value::Object(Map::new()))
            {
                for (name, schema) in fields {
                    properties
                        .entry(name.clone())
                        .or_insert_with(|| schema.clone());
                }
            }
        }
        names.extend(shared.unwrap_or_default());
    }
    if names.is_empty() {
        root.remove("required");
    } else {
        root.insert(
            "required".to_string(),
            Value::Array(names.into_iter().map(Value::String).collect()),
        );
    }
    root.insert("type".to_string(), Value::String("object".to_string()));
    Value::Object(root)
}

fn required(schema: &Value) -> BTreeSet<String> {
    schema
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

pub(crate) fn parse_args(name: &str, arguments: &str) -> Result<Value> {
    serde_json::from_str(arguments)
        .map_err(|error| Error::Tool(format!("{name} arguments are invalid: {error}")))
}

pub(in crate::tools) fn object_string<'a>(value: &'a Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Tool(format!("{field} is required")))
}

pub(in crate::tools) fn nonempty_path<'a>(args: &'a Value, tool_name: &str) -> Result<&'a str> {
    args.get("path")
        .and_then(Value::as_str)
        .filter(|path| !path.trim().is_empty())
        .ok_or_else(|| Error::Tool(format!("{tool_name} requires a non-empty path")))
}

pub(in crate::tools) fn object_usize(value: &Value, field: &str) -> Option<usize> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
}

pub(in crate::tools) fn object_bool(value: &Value, field: &str) -> bool {
    value.get(field).and_then(Value::as_bool).unwrap_or(false)
}

#[cfg(test)]
#[path = "args_tests.rs"]
mod tests;
