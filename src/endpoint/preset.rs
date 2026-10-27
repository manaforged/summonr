use std::sync::OnceLock;

use super::*;

const HOST_ID: &str = "{host_id}";
use crate::provider::Spec;

pub(super) struct Preset {
    pub(super) name: String,
    pub(super) provider: String,
    pub(super) region: Option<super::Region>,
    pub(super) replaces: Vec<String>,
    pub(super) base_url: String,
    pub(super) models_url: Option<&'static str>,
    pub(super) model: String,
    pub(super) login_model: Option<String>,
    pub(super) env_keys: &'static [&'static str],
    pub(super) env_unset: &'static [&'static str],
    pub(super) default_key: Option<&'static str>,
    pub(super) missing: String,
    pub(super) aliases: &'static [(String, String)],
    pub(super) oauth: Option<crate::config::OAuth>,
    pub(super) headers: &'static [(String, String)],
    pub(super) wire: Wire,
    pub(super) reasoning: Spec,
    pub(super) context_window: Option<u32>,
    pub(super) opencode: bool,
    pub(super) prompt_cache_key: bool,
    pub(super) assistant_after_tool: bool,
    pub(super) cache_ttl: Option<CacheTtl>,
    pub(super) cache_header: Option<String>,
    pub(super) cache_marks: bool,
    pub(super) env_default: bool,
    pub(super) plan: bool,
    pub(super) off: bool,
    pub(super) opencode_ids: &'static [&'static str],
    pub(super) label: Option<String>,
    pub(super) start: Option<String>,
    pub(super) key_from: Vec<String>,
}

pub(super) fn env_default() -> Option<&'static Preset> {
    presets().find(|preset| preset.env_default)
}

fn host_id() -> Option<String> {
    let path = crate::prompt::summonr_home().ok()?.join("host-id");
    if let Ok(found) = std::fs::read_to_string(&path)
        && !found.trim().is_empty()
    {
        return Some(found.trim().to_string());
    }
    let fresh = uuid::Uuid::now_v7().urn().to_string();
    std::fs::write(&path, &fresh).ok()?;
    Some(fresh)
}

impl Preset {
    pub(super) fn sign_in(&self) -> Option<crate::config::OAuth> {
        let mut oauth = self.oauth.clone()?;
        let host = host_id();
        oauth
            .authorize_params
            .retain(|_, value| value != HOST_ID || host.is_some());
        for value in oauth.authorize_params.values_mut() {
            if value == HOST_ID
                && let Some(host) = &host
            {
                value.clone_from(host);
            }
        }
        Some(oauth)
    }
}

fn wire(value: &str) -> std::result::Result<Wire, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "completions" => Ok(Wire::Completions),
        "responses" => Ok(Wire::Responses),
        "anthropic" => Ok(Wire::Anthropic),
        "gemini" => Ok(Wire::Gemini),
        "agent-sdk" => Ok(Wire::AgentSdk),
        other => Err(format!("unknown wire `{other}`")),
    }
}

mod row;

use row::File;

pub(super) fn table(text: &str) -> std::result::Result<Vec<Preset>, String> {
    let file: File = toml::from_str(text).map_err(|error| error.to_string())?;
    let mut seen: Vec<String> = Vec::new();
    let mut rows = Vec::new();
    for row in file.preset {
        let name = row.name.trim().to_string();
        let base_url = row.base_url.trim().to_string();
        if name.is_empty() || base_url.is_empty() {
            return Err("every preset needs a name and a base_url".to_string());
        }
        if seen.contains(&name) {
            return Err(format!("preset `{name}` is listed twice"));
        }
        seen.push(name.clone());
        rows.push(row.into_preset(name, base_url, &file.oauth)?);
    }
    let names: std::collections::BTreeSet<&str> = seen.iter().map(String::as_str).collect();
    let retired: std::collections::BTreeSet<&str> = rows
        .iter()
        .flat_map(|row| row.replaces.iter().map(String::as_str))
        .collect();
    if let Some(old) = retired.iter().find(|old| names.contains(*old)) {
        return Err(format!("`{old}` is retired but is still a preset"));
    }
    Ok(rows)
}

const PRESETS: &str = include_str!("presets.toml");

pub(super) fn listed() -> &'static [Preset] {
    static ROWS: OnceLock<Vec<Preset>> = OnceLock::new();
    ROWS.get_or_init(|| match table(PRESETS) {
        Ok(rows) => rows,
        Err(error) => panic!("the built-in provider table is invalid: {error}"),
    })
}

impl Preset {
    pub(super) const fn keyless(&self) -> bool {
        self.default_key.is_some() && self.env_keys.is_empty() && self.oauth.is_none()
    }
}

pub(super) fn presets() -> impl Iterator<Item = &'static Preset> {
    listed().iter().filter(|preset| !preset.off)
}

pub(super) fn preset(name: &str) -> Option<&'static Preset> {
    presets().find(|preset| preset.name == name)
}

pub(crate) fn catalog_providers() -> impl Iterator<Item = &'static str> {
    listed()
        .iter()
        .filter(|preset| !preset.off)
        .flat_map(|preset| preset.opencode_ids.iter().copied())
}

#[must_use]
pub(crate) fn env_unset(name: &str) -> &'static [&'static str] {
    preset(name).map_or(&[], |preset| preset.env_unset)
}

#[must_use]
pub(crate) fn provider_ids(name: &str) -> &'static [&'static str] {
    listed()
        .iter()
        .find(|preset| preset.name == name && !preset.off)
        .map_or(&[], |preset| preset.opencode_ids)
}

#[must_use]
pub(crate) fn label(name: &str) -> Option<&'static str> {
    preset(name).and_then(|preset| preset.label.as_deref())
}

#[must_use]
pub(crate) fn start_command(base: &str) -> Option<&'static str> {
    let base = trim_slash(base);
    presets()
        .find(|preset| trim_slash(&preset.base_url) == base)
        .and_then(|preset| preset.start.as_deref())
}
