//! One parser and writer for `~/.mage/config.toml`.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml_edit::{DocumentMut, value};

use crate::error::{Error, Result};
use crate::prefs::Prefs;
use crate::prompt::mage_home;
use crate::workflow::Workflow;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct File {
    pub default: Option<String>,
    #[serde(default)]
    pub endpoint: BTreeMap<String, Endpoint>,
    #[serde(default)]
    pub search: Search,
    pub theme: Option<String>,
    pub splash: Option<bool>,
    pub animations: Option<bool>,
    pub reasoning: Option<bool>,
    pub tools: Option<String>,
    pub bus: Option<bool>,
    #[serde(default)]
    pub lsp: Lsp,
    /// Default workflow name.
    pub workflow: Option<String>,
    /// `[workflows.NAME]`: role -> seat rows.
    #[serde(default)]
    pub workflows: BTreeMap<String, BTreeMap<String, SeatRow>>,
}

/// `role = "endpoint"` or `role = { endpoint = "…", model = "…" }`.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum SeatRow {
    Name(String),
    Full {
        endpoint: String,
        model: Option<String>,
    },
}

/// Language server argv. Empty means detect from the workspace tree.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct Lsp {
    pub command: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Endpoint {
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    /// OAuth token endpoint. When set, the connector fetches a bearer
    /// token; `api_key` is not used.
    pub auth: Option<OAuth>,
    pub body: Option<toml::Value>,
    pub context_window: Option<u32>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Extra HTTP headers on every chat request (Copilot editor headers, …).
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// `anthropic`, `gemini`, or omitted for chat/completions.
    pub wire: Option<String>,
}

/// RFC 6749 token request for a `chat/completions` host.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct OAuth {
    pub token_url: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
    /// `client_credentials`, `refresh_token`, or `device_code`.
    #[serde(default)]
    pub grant: Option<String>,
    /// RFC 8628 device authorization URL. Implies `device_code`.
    #[serde(default)]
    pub device_url: Option<String>,
    /// After the OAuth access token, GET this URL with that Bearer and
    /// use the JSON `token` / `access_token` as the chat bearer.
    #[serde(default)]
    pub exchange_url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Search {
    pub count: Option<usize>,
    pub allowed_domains: Option<Vec<String>>,
    pub excluded_domains: Option<Vec<String>>,
}

pub(crate) fn path() -> Result<PathBuf> {
    Ok(mage_home()?.join("config.toml"))
}

pub(crate) fn load() -> Result<File> {
    load_from(&path()?)
}

pub(crate) fn load_from(path: &Path) -> Result<File> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(File::default()),
        Err(error) => return Err(error.into()),
    };
    let file: File =
        toml::from_str(&text).map_err(|error| Error::Config(format!("config.toml: {error}")))?;
    if file
        .tools
        .as_deref()
        .is_some_and(|tools| !matches!(tools, "compact" | "full"))
    {
        return Err(Error::Config(
            "config.toml: `tools` must be `compact` or `full`".to_string(),
        ));
    }
    if file
        .theme
        .as_deref()
        .is_some_and(|theme| !matches!(theme, "space" | "fantasy"))
    {
        return Err(Error::Config(
            "config.toml: `theme` must be `space` or `fantasy`".to_string(),
        ));
    }
    Ok(file)
}

pub(crate) fn save_prefs(path: &Path, prefs: &Prefs) -> Result<()> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let mut document = text
        .parse::<DocumentMut>()
        .map_err(|error| Error::Config(format!("config.toml: {error}")))?;
    document["theme"] = value(&prefs.theme);
    document["splash"] = value(prefs.splash);
    document["animations"] = value(prefs.animations);
    document["reasoning"] = value(prefs.reasoning);
    document["tools"] = value(prefs.tools.name());
    document["bus"] = value(prefs.bus);
    persist(path, &document)
}

pub(crate) fn save_key(path: &Path, name: &str, key: &str) -> Result<()> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let mut document = text
        .parse::<DocumentMut>()
        .map_err(|error| Error::Config(format!("config.toml: {error}")))?;
    document["endpoint"][name]["api_key"] = value(key);
    persist(path, &document)
}

pub(crate) fn save_workflow(path: &Path, workflow: &Workflow) -> Result<()> {
    let mut document = read_document(path)?;
    let mut table = toml_edit::Table::new();
    for (role, seat) in &workflow.seats {
        table[role.name()] = match &seat.model {
            None => value(&seat.endpoint),
            Some(model) => {
                let mut row = toml_edit::InlineTable::new();
                row.insert("endpoint", seat.endpoint.as_str().into());
                row.insert("model", model.as_str().into());
                toml_edit::Item::Value(toml_edit::Value::InlineTable(row))
            }
        };
    }
    document["workflows"][workflow.name.as_str()] = toml_edit::Item::Table(table);
    if let Some(parent) = document["workflows"].as_table_mut() {
        parent.set_implicit(true);
    }
    persist(path, &document)
}

pub(crate) fn save_default_workflow(path: &Path, name: &str) -> Result<()> {
    let mut document = read_document(path)?;
    document["workflow"] = value(name);
    persist(path, &document)
}

fn read_document(path: &Path) -> Result<DocumentMut> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    text.parse::<DocumentMut>()
        .map_err(|error| Error::Config(format!("config.toml: {error}")))
}

fn persist(path: &Path, document: &DocumentMut) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Config("config.toml has no parent directory".to_string()))?;
    std::fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(document.to_string().as_bytes())?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| Error::Io(error.error))?;
    Ok(())
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
