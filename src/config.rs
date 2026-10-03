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
    pub last_endpoint: Option<String>,
    #[serde(default)]
    pub endpoint: BTreeMap<String, Endpoint>,
    #[serde(default)]
    pub search: Search,
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
    pub ban_comments: Option<bool>,
    pub ban_tests: Option<bool>,
    pub plan_mode: Option<bool>,
    #[serde(default)]
    pub hotkeys: BTreeMap<String, String>,
    pub secrets: Option<String>,
    #[serde(default)]
    pub lsp: Lsp,
    #[serde(default)]
    pub sandbox: Sandbox,
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
    #[serde(default)]
    pub hooks: crate::hooks::Hooks,
    #[serde(default)]
    pub plugins: crate::plugins::Switches,
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

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct Sandbox {
    #[serde(default)]
    pub on: bool,
    #[serde(default)]
    pub write: Vec<String>,
}

impl Sandbox {
    pub(crate) fn paths(&self) -> Vec<PathBuf> {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);
        self.write
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
            .map_or(crate::limits::SESSION_RETAIN, |days| {
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
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct Search {
    pub count: Option<usize>,
    pub allowed_domains: Option<Vec<String>>,
    pub excluded_domains: Option<Vec<String>>,
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
        store::write(path, document.to_string())?;
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
            crate::limits::AUTH.as_secs()
        ))
    };
    let (send, wait) = std::sync::mpsc::channel();
    let owned = path.to_path_buf();
    std::thread::spawn(move || drop(send.send(store::lock(&owned, true))));
    match wait.recv_timeout(crate::limits::AUTH) {
        Ok(held) => held?.ok_or_else(locked),
        Err(_) => Err(locked()),
    }
}

pub fn check() -> Result<()> {
    file().map(drop)
}

pub fn sandbox_on() -> Result<bool> {
    Ok(file()?.sandbox.on)
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
pub(crate) fn save_last_endpoint(path: &Path, name: &str) -> Result<bool> {
    update(path, |document| {
        if document
            .get("last_endpoint")
            .and_then(toml_edit::Item::as_str)
            == Some(name)
        {
            return Ok(false);
        }
        document["last_endpoint"] = value(name);
        Ok(true)
    })
}

pub(crate) use edit::{
    remove_key, remove_mcp_server, sandbox_entry, save_default_seat, save_default_workflow,
    save_mcp_server, save_module, save_prefs, save_sandbox_write, save_workflow,
};

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
