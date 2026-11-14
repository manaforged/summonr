use std::fmt::Display;
use std::path::Path;

use serde::{Deserialize, Deserializer};
use serde_json::Value;

use summonr_core::error::{Error, Result};

/// `what` prefixed with the file it is about.
pub(crate) fn located(path: &Path, what: impl Display) -> String {
    format!("{}: {what}", path.display())
}

pub(crate) fn is_json(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension == "json")
}

/// Server entry `key` from `file` read as `T`, or a message naming the file and the key.
pub(crate) fn server_entry<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
    file: &Path,
    key: &str,
    entry: D,
) -> std::result::Result<T, String> {
    T::deserialize(entry).map_err(|error| located(file, format_args!("{key}: {error}")))
}

/// Parses `path` when it holds text; a missing or blank file is `None`.
pub(crate) fn read_parsed<T, E: Display>(
    path: &Path,
    parse: impl FnOnce(&str) -> std::result::Result<T, E>,
) -> Result<Option<T>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(Error::Config(located(path, error))),
    };
    if text.trim().is_empty() {
        return Ok(None);
    }
    parse(&text)
        .map(Some)
        .map_err(|error| Error::Config(located(path, error)))
}

/// Parses `path` like `read_parsed`, and on an error prints one note naming the file and skips it.
pub(crate) fn read_noted<T, E: Display>(
    path: &Path,
    parse: impl FnOnce(&str) -> std::result::Result<T, E>,
) -> Option<T> {
    read_parsed(path, parse).unwrap_or_else(|error| {
        skip(&error.detail());
        None
    })
}

/// Prints the note for a plugin file summonr skips, `problem` naming the file and what is wrong.
pub(crate) fn skip(problem: &str) {
    crate::display::note(&format!("{problem}; summonr skipped this plugin file"));
}

pub(crate) fn json_file(path: &Path) -> Result<Value> {
    Ok(read_parsed(path, |text: &str| serde_json::from_str::<Value>(text))?.unwrap_or(Value::Null))
}

/// The JSON in `path`, or `null` when it is missing, blank, or broken; a broken file prints a note.
pub(crate) fn read_json(path: &Path) -> Value {
    read_noted(path, |text: &str| serde_json::from_str::<Value>(text)).unwrap_or(Value::Null)
}
