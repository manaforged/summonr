use std::path::Path;

use serde_json::{Map, Value};
use summonr_core::error::{Error, Result};

use super::catalog::Entry;
use crate::durable::store::{self, Durability};
use crate::plugins::{HOOKS, MANIFEST, json_file};

const COMPONENTS: [&str; 6] = [
    "commands",
    "agents",
    "skills",
    HOOKS,
    "outputStyles",
    "themes",
];
const DISPLAY: [&str; 8] = [
    "displayName",
    "description",
    "author",
    "homepage",
    "repository",
    "license",
    "keywords",
    "defaultEnabled",
];
const ENTRY_ONLY: [&str; 8] = [
    "source",
    "strict",
    "category",
    "tags",
    "relevance",
    "metadata",
    "headers",
    "headersHelper",
];
const VERSION: &str = "version";
const NAME: &str = "name";

/// Writes the manifest the entry and the fetched `plugin.json` make together into `dir`.
pub(super) fn write(entry: &Entry, dir: &Path) -> Result<()> {
    let path = dir.join(MANIFEST);
    let found = match json_file(&path)? {
        Value::Object(manifest) => Some(manifest),
        Value::Null => None,
        other => {
            return Err(Error::Config(format!(
                "{}: a manifest is a JSON object, not {other}",
                path.display()
            )));
        }
    };
    let mut merged = manifest(entry, found)?;
    merged
        .entry(NAME)
        .or_insert_with(|| Value::String(entry.name.clone()));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    store::write(
        &path,
        serde_json::to_vec_pretty(&Value::Object(merged))?,
        Durability::Renamed,
    )?;
    Ok(())
}

fn manifest(entry: &Entry, found: Option<Map<String, Value>>) -> Result<Map<String, Value>> {
    if entry
        .fields
        .get(HOOKS)
        .is_some_and(|hooks| !hooks.is_object())
    {
        return Err(Error::Config(format!(
            "plugin `{}`: hooks as a path or a list are not yet supported in a marketplace entry; write them inline",
            entry.name
        )));
    }
    let Some(mut manifest) = found else {
        let mut manifest = entry.fields.clone();
        manifest.retain(|key, _| !ENTRY_ONLY.contains(&key.as_str()));
        return Ok(manifest);
    };
    let declared: Vec<&str> = COMPONENTS
        .into_iter()
        .filter(|key| entry.fields.contains_key(*key))
        .collect();
    if !entry.strict && !declared.is_empty() {
        return Err(Error::Config(format!(
            "Plugin {} has conflicting manifests: both plugin.json and marketplace entry specify components",
            entry.name
        )));
    }
    for key in declared {
        let add = &entry.fields[key];
        if key == HOOKS {
            replace_events(&mut manifest, add);
        } else {
            append(&mut manifest, key, add);
        }
    }
    for key in DISPLAY {
        if let Some(value) = entry.fields.get(key) {
            manifest.insert(key.to_string(), value.clone());
        }
    }
    if let Some(version) = entry.fields.get(VERSION) {
        manifest.entry(VERSION).or_insert_with(|| version.clone());
    }
    Ok(manifest)
}

fn append(manifest: &mut Map<String, Value>, key: &str, add: &Value) {
    let merged = match (manifest.remove(key), add) {
        (None, add) => add.clone(),
        (Some(Value::Object(mut have)), Value::Object(more)) => {
            have.extend(more.clone());
            Value::Object(have)
        }
        (Some(have), add) => {
            Value::Array(items(have).into_iter().chain(items(add.clone())).collect())
        }
    };
    manifest.insert(key.to_string(), merged);
}

fn replace_events(manifest: &mut Map<String, Value>, add: &Value) {
    match (manifest.get_mut(HOOKS), add) {
        (Some(Value::Object(have)), Value::Object(events)) => {
            have.extend(events.clone());
        }
        (None, add) => {
            manifest.insert(HOOKS.to_string(), add.clone());
        }
        (Some(have), add) => {
            let mut list = items(have.take());
            list.push(add.clone());
            *have = Value::Array(list);
        }
    }
}

fn items(value: Value) -> Vec<Value> {
    match value {
        Value::Array(many) => many,
        one => vec![one],
    }
}
