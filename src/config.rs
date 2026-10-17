use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use serde::Deserialize;
use toml_edit::{DocumentMut, value};

use crate::durable::store;
use crate::error::{Error, Result};
use crate::prefs::Prefs;
use crate::prompt::summonr_home;
use crate::workflow::Workflow;

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct File {
    pub default: Option<String>,
    pub default_model: Option<String>,
    #[serde(default)]
    pub endpoint: BTreeMap<String, Endpoint>,
    #[serde(default)]
    pub provider: BTreeMap<String, crate::endpoint::Choice>,
    #[serde(default)]
    pub search: Search,
    #[serde(default)]
    pub browser: Browser,
    pub theme: Option<String>,
    pub background: Option<crate::prefs::Background>,
    pub notify: Option<crate::prefs::Notify>,
    pub splash: Option<bool>,
    pub animations: Option<bool>,
    pub fps: Option<u32>,
    pub reasoning: Option<ReasoningSetting>,
    pub tools: Option<crate::prefs::ToolOutput>,
    pub diff_layout: Option<crate::prefs::DiffLayout>,
    pub receipts: Option<String>,
    #[serde(alias = "bus")]
    pub mail: Option<bool>,
    pub layout: Option<String>,
    #[serde(default)]
    pub hotkeys: BTreeMap<String, String>,
    pub secrets: Option<String>,
    #[serde(default)]
    pub lsp: Lsp,
    #[serde(default)]
    pub write_roots: Vec<String>,
    pub workflow: Option<String>,
    #[serde(default)]
    pub workflows: BTreeMap<String, BTreeMap<String, SeatRow>>,
    #[serde(default)]
    #[serde(deserialize_with = "crate::modules::switch_map")]
    pub modules: BTreeMap<crate::modules::ModuleId, bool>,
    #[serde(default)]
    pub sessions: Sessions,
    #[serde(default)]
    pub mcp: BTreeMap<String, crate::mcp::Entry>,
    #[serde(default)]
    pub import_mcp: Option<bool>,
    pub models_catalog: Option<bool>,
    #[serde(default)]
    pub hooks: crate::hooks::Hooks,
    #[serde(default)]
    pub plugins: crate::plugins::Switches,
    pub limits: Option<crate::limits::Overrides>,
    #[serde(flatten)]
    pub other: BTreeMap<String, toml::Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum SeatRow {
    Name(String),
    Full {
        endpoint: String,
        model: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ReasoningSetting {
    Flag(bool),
    Text(String),
}

impl File {
    pub(crate) fn write_root_paths(&self) -> Vec<PathBuf> {
        let home = home_dir();
        self.write_roots
            .iter()
            .map(|path| match (path.strip_prefix("~/"), &home) {
                (Some(rest), Some(home)) => home.join(rest),
                _ => PathBuf::from(path),
            })
            .collect()
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct Sessions {
    pub retain_days: Option<std::num::NonZeroU32>,
}

impl Sessions {
    pub(crate) fn retain(&self) -> std::time::Duration {
        self.retain_days
            .map_or(crate::limits::limits().session_retain(), |days| {
                std::time::Duration::from_secs(crate::session::DAY * u64::from(days.get()))
            })
    }
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct Lsp {
    pub module: Option<String>,
    pub command: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct Endpoint {
    pub base_url: Option<String>,
    pub models_url: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub auth: Option<OAuth>,
    pub body: Option<toml::Value>,
    pub context_window: Option<u32>,
    pub idle_timeout_secs: Option<u64>,
    pub effort: Option<crate::provider::Effort>,
    pub price: Option<crate::provider::Price>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "decode::wire")]
    pub wire: Option<crate::endpoint::Wire>,
    #[serde(default)]
    pub fallback: Vec<String>,
    pub compact_tokens: Option<u32>,
    pub images: Option<bool>,
    pub keep_reasoning: Option<bool>,
    pub cache_ttl: Option<crate::endpoint::CacheTtl>,
    pub reasoning_levels: Option<Vec<crate::provider::Effort>>,
    pub thinking_off: Option<crate::provider::Off>,
    #[serde(default)]
    pub models: BTreeMap<String, Model>,
    #[serde(flatten)]
    pub other: BTreeMap<String, toml::Value>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct Model {
    pub body: Option<toml::Value>,
    pub context_window: Option<u32>,
    pub effort: Option<crate::provider::Effort>,
    pub compact_tokens: Option<u32>,
    pub keep_reasoning: Option<bool>,
    pub reasoning_levels: Option<Vec<crate::provider::Effort>>,
    pub thinking_off: Option<crate::provider::Off>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
pub struct OAuth {
    pub token_url: String,
    pub client_id: String,
    #[serde(default)]
    pub label: Option<String>,
    pub client_secret: Option<String>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
    #[serde(default)]
    pub grant: Option<String>,
    #[serde(default)]
    pub device_url: Option<String>,
    #[serde(default)]
    pub exchange_url: Option<String>,
    #[serde(default)]
    pub authorize_url: Option<String>,
    #[serde(default)]
    pub redirect_uri: Option<String>,
    #[serde(default)]
    pub authorize_params: BTreeMap<String, String>,
    #[serde(default)]
    pub token_params: BTreeMap<String, String>,
    #[serde(default)]
    pub token_style: Option<String>,
    #[serde(default)]
    pub scheme: Option<String>,
    #[serde(default)]
    pub pkce: bool,
    #[serde(default)]
    pub claim_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub required_scope: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct Search {
    pub count: Option<usize>,
    pub allowed_domains: Option<Vec<String>>,
    pub excluded_domains: Option<Vec<String>>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct Browser {
    pub profile: Option<String>,
    pub headed: Option<bool>,
    pub chrome: Option<PathBuf>,
    pub events: Option<BrowserEvents>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub(crate) struct BrowserEvents {
    #[serde(default = "events_on")]
    pub page: bool,
    #[serde(default = "events_on")]
    pub runtime: bool,
}

const fn events_on() -> bool {
    true
}

pub(crate) fn path() -> Result<PathBuf> {
    Ok(summonr_home()?.join("config.toml"))
}

static PARSED: RwLock<Option<Arc<File>>> = RwLock::new(None);

pub(crate) fn file() -> Result<Arc<File>> {
    if let Some(file) = PARSED.read().ok().and_then(|cached| cached.clone()) {
        return Ok(file);
    }
    let file = Arc::new(parse(&path()?)?);
    drop(
        PARSED
            .write()
            .map(|mut cached| *cached = Some(Arc::clone(&file))),
    );
    Ok(file)
}

fn update(path: &Path, change: impl FnOnce(&mut DocumentMut) -> Result<bool>) -> Result<bool> {
    let held = hold(path)?;
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let mut document = text
        .parse::<DocumentMut>()
        .map_err(|error| Error::Config(format!("{}: {error}", tilde(path))))?;
    let changed = change(&mut document)?;
    if changed {
        store::write(path, document.to_string(), store::Durability::Synced)?;
        drop(PARSED.write().map(|mut cached| *cached = None));
    }
    drop(held);
    Ok(changed)
}

pub(crate) fn hold(path: &Path) -> Result<std::fs::File> {
    if let Some(file) = store::lock(path, false)? {
        return Ok(file);
    }
    let locked = || {
        Error::Config(format!(
            "{} is locked by another summonr process; gave up after {}s",
            tilde(path),
            crate::limits::limits().auth_secs
        ))
    };
    let (send, wait) = std::sync::mpsc::channel();
    let owned = path.to_path_buf();
    std::thread::spawn(move || drop(send.send(store::lock(&owned, true))));
    match wait.recv_timeout(crate::limits::limits().auth()) {
        Ok(held) => held?.ok_or_else(locked),
        Err(_) => Err(locked()),
    }
}

pub fn check() -> Result<()> {
    file().map(drop)
}

pub(crate) fn parse(path: &Path) -> Result<File> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(File::default()),
        Err(error) => return Err(error.into()),
    };
    let file = decode(path, &text)?;
    warn_unknown(path, &file);
    validate(path, &file)?;
    Ok(file)
}

mod decode;
mod edit;
use decode::{decode, validate, warn_unknown};
pub use edit::tilde;

pub(crate) fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub(crate) fn expand_tilde(word: &str) -> Option<PathBuf> {
    if matches!(word, "~" | "$HOME" | "${HOME}") {
        return home_dir();
    }
    let rest = word
        .strip_prefix("~/")
        .or_else(|| word.strip_prefix("$HOME/"))
        .or_else(|| word.strip_prefix("${HOME}/"))?;
    Some(home_dir()?.join(rest))
}

pub(crate) use edit::{
    Section, drop_plugin_off, remove_key, remove_named, save_default_seat, save_default_workflow,
    save_module, save_named, save_prefs, save_provider, save_workflow, save_write_root,
    write_root_entry,
};

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
