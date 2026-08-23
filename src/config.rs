//! One parser and writer for `~/.mage/config.toml`.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml_edit::{DocumentMut, value};

use crate::error::{Error, Result};
use crate::prefs::Prefs;
use crate::prompt::mage_home;

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
