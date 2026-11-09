use serde_json::Value;
use summonr_core::error::{Error, Result};
pub(crate) use summonr_types::tool;

pub(crate) fn parse_args(name: &str, arguments: &str) -> Result<Value> {
    serde_json::from_str(arguments).map_err(|error| invalid(name, &error))
}

pub(in crate::tools) fn typed<'a, T: serde::Deserialize<'a>>(
    name: &str,
    args: &'a Value,
) -> Result<T> {
    T::deserialize(args).map_err(|error| invalid(name, &error))
}

fn invalid(name: &str, error: &serde_json::Error) -> Error {
    Error::Tool(format!("{name} arguments are invalid: {error}"))
}

pub(in crate::tools) fn nonblank(text: Option<&str>) -> Option<&str> {
    text.filter(|text| !text.trim().is_empty())
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

pub(in crate::tools) fn named<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    nonblank(value.get(field).and_then(Value::as_str))
}

pub(in crate::tools) fn named_value(value: &Value) -> bool {
    value.as_str().is_some_and(|text| !text.trim().is_empty())
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
