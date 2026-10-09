use std::path::Path;

use serde_json::Value;

use crate::error::{Error, Result};

const UNITS: &[(char, u64)] = &[('m', 60), ('h', 3_600), ('d', 86_400), ('w', 604_800)];
const GROUPS: &[&str] = &["folder", "dir", "directory"];

pub(super) fn name_words(args: &Value) -> Vec<String> {
    args.get("name")
        .and_then(Value::as_str)
        .map(|name| {
            name.split_whitespace()
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

pub(super) fn named(words: &[String], path: &Path) -> bool {
    if words.is_empty() {
        return true;
    }
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    words.iter().all(|word| name.contains(word.as_str()))
}

pub(super) fn newer(args: &Value) -> Result<Option<u64>> {
    let Some(text) = args
        .get("newer")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
    else {
        return Ok(None);
    };
    let invalid = || {
        Error::Tool(format!(
            "search newer `{text}` is invalid: use a number and m, h, d, or w, such as 2d"
        ))
    };
    let unit = text.chars().last().ok_or_else(invalid)?;
    let scale = UNITS
        .iter()
        .find(|(name, _)| *name == unit.to_ascii_lowercase())
        .map(|(_, seconds)| *seconds)
        .ok_or_else(invalid)?;
    let count: u64 = text[..text.len() - unit.len_utf8()]
        .trim()
        .parse()
        .map_err(|_parse: std::num::ParseIntError| invalid())?;
    Ok(Some(
        crate::mail::now().saturating_sub(count.saturating_mul(scale)),
    ))
}

pub(super) fn grouped(args: &Value) -> Result<bool> {
    match args.get("group") {
        None | Some(Value::Null) => Ok(false),
        Some(Value::Bool(on)) => Ok(*on),
        Some(Value::String(by)) if GROUPS.contains(&by.as_str()) => Ok(true),
        Some(other) => Err(Error::Tool(format!(
            "search group {other} is invalid: use \"folder\""
        ))),
    }
}

pub(super) fn folder(path: &Path) -> String {
    path.parent().unwrap_or(path).to_string_lossy().into_owned()
}
