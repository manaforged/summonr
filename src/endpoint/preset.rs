use std::collections::BTreeMap;
use std::sync::OnceLock;

use super::*;
use crate::provider::Spec;
use summonr_core::catalog::{Catalog, Row};

const HOST_ID: &str = "{host_id}";

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Preset {
    pub(super) name: String,
    pub(super) base_url: String,
    #[serde(default)]
    pub(super) provider: String,
    #[serde(default)]
    pub(super) region: Option<super::Region>,
    #[serde(default)]
    pub(super) replaces: Vec<String>,
    #[serde(default)]
    pub(super) models_url: Option<String>,
    #[serde(default)]
    pub(super) model: String,
    #[serde(default)]
    pub(super) login_model: Option<String>,
    #[serde(default)]
    pub(super) env_keys: Vec<String>,
    #[serde(default)]
    pub(super) env_unset: Vec<String>,
    #[serde(default)]
    pub(super) default_key: Option<String>,
    #[serde(default)]
    pub(super) missing: String,
    #[serde(default)]
    pub(super) aliases: BTreeMap<String, String>,
    #[serde(default, rename = "oauth")]
    login: Option<String>,
    #[serde(skip)]
    pub(super) oauth: Option<crate::config::OAuth>,
    #[serde(default)]
    pub(super) headers: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "row::wire")]
    pub(super) wire: Wire,
    #[serde(default, rename = "reasoning")]
    reasoning_row: row::ReasoningRow,
    #[serde(skip)]
    pub(super) reasoning: Spec,
    #[serde(default)]
    pub(super) context_window: Option<u32>,
    #[serde(default)]
    pub(super) session_headers: BTreeMap<String, SessionValue>,
    #[serde(default)]
    pub(super) keep_reasoning: bool,
    #[serde(default)]
    pub(super) prompt_cache_key: bool,
    #[serde(default)]
    pub(super) assistant_after_tool: bool,
    #[serde(default)]
    pub(super) cache_ttl: Option<CacheTtl>,
    #[serde(default)]
    pub(super) cache_marks: bool,
    #[serde(default)]
    pub(super) env_default: bool,
    #[serde(default)]
    pub(super) plan: bool,
    #[serde(default)]
    pub(super) off: bool,
    #[serde(default)]
    pub(super) opencode_ids: Vec<String>,
    #[serde(default)]
    pub(super) found: Option<Found>,
    #[serde(default)]
    pub(super) label: Option<String>,
    #[serde(default)]
    pub(super) start: Option<String>,
    #[serde(default)]
    pub(super) key_from: Vec<String>,
}

impl Row for Preset {
    fn name(&self) -> &str {
        &self.name
    }

    fn retired(&self) -> &[String] {
        &self.replaces
    }
}

pub(super) fn env_default() -> Option<&'static Preset> {
    presets().find(|preset| preset.env_default)
}

fn host_id() -> Option<String> {
    let path = summonr_core::dirs::summonr_home().ok()?.join("host-id");
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

    pub(super) const fn keyless(&self) -> bool {
        self.default_key.is_some() && self.env_keys.is_empty() && self.oauth.is_none()
    }
}

mod row;

use row::File;
pub(crate) use row::wire;

pub(super) fn table(text: &str) -> std::result::Result<Catalog<Preset>, String> {
    let File { mut preset, oauth } = summonr_core::table::parse(text)?;
    for row in &mut preset {
        row.check(&oauth)?;
    }
    Catalog::from_rows("provider preset", preset)
}

const PRESETS: &str = include_str!("presets.toml");

pub(super) fn listed() -> &'static Catalog<Preset> {
    static ROWS: OnceLock<Catalog<Preset>> = OnceLock::new();
    ROWS.get_or_init(|| match table(PRESETS) {
        Ok(rows) => rows,
        Err(error) => panic!("the built-in provider table is invalid: {error}"),
    })
}

/// How a local provider shows it is installed on this machine, and the name a user knows it by.
#[derive(serde::Deserialize, Clone, Debug)]
pub(super) struct Found {
    pub(super) name: String,
    #[serde(flatten)]
    pub(super) probe: Probe,
}

/// What to look for: a program on PATH or a server on a local port.
#[derive(serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "lowercase")]
pub(super) enum Probe {
    Program(String),
    Port(u16),
}

impl Probe {
    pub(super) fn here(&self) -> bool {
        match self {
            Self::Program(name) => crate::askpass::on_path(name).is_some(),
            Self::Port(port) => std::net::TcpStream::connect_timeout(
                &std::net::SocketAddr::from(([127, 0, 0, 1], *port)),
                crate::limits::limits().local_probe(),
            )
            .is_ok(),
        }
    }
}

pub(super) fn presets() -> impl Iterator<Item = &'static Preset> {
    listed().rows().iter().filter(|preset| !preset.off)
}

pub(super) fn preset(name: &str) -> Option<&'static Preset> {
    presets().find(|preset| preset.name == name)
}

pub(crate) fn catalog_providers() -> impl Iterator<Item = &'static str> {
    presets().flat_map(|preset| preset.opencode_ids.iter().map(String::as_str))
}

#[must_use]
pub(crate) fn env_unset(name: &str) -> &'static [String] {
    preset(name).map_or(&[], |preset| &preset.env_unset)
}

#[must_use]
pub(crate) fn provider_ids(name: &str) -> &'static [String] {
    preset(name).map_or(&[], |preset| &preset.opencode_ids)
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
