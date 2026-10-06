use std::path::Path;

use serde::{Deserialize, Deserializer};

use super::{File, tilde};
use crate::endpoint::Wire;
use crate::error::{Error, Result};

static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(super) fn wire<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<Wire>, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.map(|name| Wire::parse(&name)))
}

fn bad(path: &Path, message: &str) -> Error {
    Error::Config(format!("{}: {message}", tilde(path)))
}

pub(super) fn decode(path: &Path, text: &str) -> Result<File> {
    let mut table: toml::Table = toml::from_str(text).map_err(|error| {
        let line = error.span().map_or(1, |span| {
            text.get(..span.start)
                .map_or(1, |head| head.matches('\n').count() + 1)
        });
        bad(
            path,
            &format!(
                "line {line}: {}; fix that line, or move the file aside to start fresh",
                error.message().trim().replace('\n', "; ")
            ),
        )
    })?;
    if table.contains_key("mail") {
        table.remove("bus");
    }
    table
        .try_into()
        .map_err(|error: toml::de::Error| bad(path, &error.message().trim().replace('\n', "; ")))
}

pub(super) fn warn_unknown(path: &Path, file: &File) {
    let unknown: Vec<String> = file
        .other
        .keys()
        .cloned()
        .chain(file.endpoint.iter().flat_map(|(name, row)| {
            row.other
                .keys()
                .map(move |key| format!("endpoint.{name}.{key}"))
        }))
        .collect();
    if !unknown.is_empty() && !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        eprintln!(
            "summonr: {} has unknown key{} {}. Check the spelling; summonr ignores {}",
            tilde(path),
            if unknown.len() == 1 { "" } else { "s" },
            unknown.join(", "),
            if unknown.len() == 1 { "it" } else { "them" }
        );
    }
}

pub(super) fn validate(path: &Path, file: &File) -> Result<()> {
    if file
        .theme
        .as_deref()
        .is_some_and(|theme| !crate::prefs::known_theme(theme))
    {
        return Err(bad(path, "`theme` must be space, nord, dawn, or auto"));
    }
    if file
        .fps
        .is_some_and(|fps| !(crate::prefs::FPS_MIN..=crate::prefs::FPS_MAX).contains(&fps))
    {
        return Err(bad(
            path,
            &format!(
                "`fps` must be a whole number from {} to {}",
                crate::prefs::FPS_MIN,
                crate::prefs::FPS_MAX
            ),
        ));
    }
    if let Some(other) = file.secrets.as_deref().map(str::trim)
        && !matches!(other, "" | "file" | "keychain")
    {
        return Err(bad(
            path,
            &format!("secrets must be \"file\" or \"keychain\", not `{other}`"),
        ));
    }
    if file.lsp.module.is_some() && file.lsp.command.is_some() {
        return Err(bad(path, "[lsp] accepts `module` or `command`, not both"));
    }
    if let Some(module) = file.lsp.module.as_deref() {
        crate::language::named(module)?;
    }
    if file.lsp.command.as_ref().is_some_and(Vec::is_empty) {
        return Err(bad(path, "[lsp] command must not be empty"));
    }
    Ok(())
}
