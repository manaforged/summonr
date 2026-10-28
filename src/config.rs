use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Deserialize;
use toml_edit::{DocumentMut, value};

use crate::durable::store;
use crate::prefs::Prefs;
use crate::workflow::Workflow;
use summonr_core::dirs::{home_dir, summonr_home};
use summonr_core::error::{Error, Result};

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct File {
    pub default: Option<String>,
    pub default_model: Option<String>,
    #[serde(default)]
    pub endpoint: BTreeMap<String, Endpoint>,
    #[serde(default)]
    pub provider: BTreeMap<String, summonr_types::endpoint::Choice>,
    #[serde(default)]
    pub search: Search,
    #[serde(default)]
    pub browser: Browser,
    pub theme: Option<String>,
    pub background: Option<summonr_types::Background>,
    pub notify: Option<summonr_types::Notify>,
    pub splash: Option<bool>,
    pub animations: Option<bool>,
    pub fps: Option<u32>,
    pub reasoning: Option<ReasoningSetting>,
    pub tools: Option<summonr_types::prefs::ToolOutput>,
    pub diff_layout: Option<summonr_types::DiffLayout>,
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
    pub workflows: BTreeMap<String, BTreeMap<String, ModelSelectionRow>>,
    #[serde(default)]
    #[serde(deserialize_with = "summonr_types::modules::switch_map")]
    pub modules: BTreeMap<summonr_types::modules::ModuleId, bool>,
    #[serde(default)]
    pub sessions: Sessions,
    #[serde(default)]
    pub mcp: BTreeMap<String, summonr_types::mcp::Entry>,
    #[serde(default)]
    pub import_mcp: Option<bool>,
    pub models_catalog: Option<bool>,
    #[serde(default)]
    pub hooks: summonr_types::hooks::Hooks,
    #[serde(default)]
    pub plugins: summonr_types::plugins::Switches,
    pub limits: Option<crate::limits::Overrides>,
    #[serde(flatten)]
    pub other: BTreeMap<String, toml::Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ModelSelectionRow {
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
    pub effort: Option<summonr_types::Effort>,
    pub price: Option<summonr_types::Price>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default, deserialize_with = "decode::wire")]
    pub wire: Option<summonr_types::endpoint::Wire>,
    #[serde(default)]
    pub fallback: Vec<String>,
    pub compact_tokens: Option<u32>,
    pub images: Option<bool>,
    pub keep_reasoning: Option<bool>,
    pub cache_ttl: Option<summonr_types::endpoint::CacheTtl>,
    pub reasoning_levels: Option<Vec<summonr_types::Effort>>,
    pub thinking_off: Option<summonr_types::Off>,
    #[serde(default)]
    pub models: BTreeMap<String, Model>,
    #[serde(flatten)]
    pub other: BTreeMap<String, toml::Value>,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub(crate) struct Model {
    pub body: Option<toml::Value>,
    pub context_window: Option<u32>,
    pub effort: Option<summonr_types::Effort>,
    pub compact_tokens: Option<u32>,
    pub keep_reasoning: Option<bool>,
    pub reasoning_levels: Option<Vec<summonr_types::Effort>>,
    pub thinking_off: Option<summonr_types::Off>,
}

pub(crate) use summonr_types::OAuth;

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

pub(crate) fn file() -> Result<Arc<File>> {
    doc::<Parsed>(&path()?).current()
}

struct Parsed;

impl store::Codec for Parsed {
    type Value = File;

    fn decode(path: &Path, text: &str) -> Result<File> {
        let file = decode(path, text)?;
        warn_unknown(path, &file);
        validate(path, &file)?;
        Ok(file)
    }
}

struct Edit;

impl store::Codec for Edit {
    type Value = DocumentMut;

    fn decode(path: &Path, text: &str) -> Result<DocumentMut> {
        text.parse::<DocumentMut>()
            .map_err(|error| Error::Config(format!("{}: {error}", tilde(path))))
    }
}

impl store::Encode for Edit {
    fn encode(document: &DocumentMut) -> Result<String> {
        Ok(document.to_string())
    }
}

fn lock_wait() -> store::Wait {
    store::Wait::Within(|| crate::limits::limits().auth())
}

fn doc<C: store::Codec>(path: &Path) -> store::Doc<C>
where
    C::Value: Default,
{
    store::Doc::new(
        path.to_path_buf(),
        store::Durability::Synced,
        store::Invalid::Fail,
        lock_wait(),
    )
}

fn update(path: &Path, change: impl FnOnce(&mut DocumentMut) -> Result<bool>) -> Result<bool> {
    doc::<Edit>(path).update(change)
}

pub(crate) fn hold(path: &Path) -> Result<std::fs::File> {
    store::hold(path, lock_wait())
}

pub fn check() -> Result<()> {
    file().map(drop)
}

pub(crate) fn parse(path: &Path) -> Result<File> {
    doc::<Parsed>(path).load()
}

mod decode;
mod edit;
use decode::{decode, validate, warn_unknown};
pub use edit::tilde;

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
    Section, drop_plugin_off, remove_key, remove_named, save_default_model, save_default_workflow,
    save_module, save_named, save_prefs, save_provider, save_workflow, save_write_root,
    write_root_entry,
};

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
