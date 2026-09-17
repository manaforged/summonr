use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml_edit::{DocumentMut, value};

use crate::durable::store;
use crate::error::{Error, Result};
use crate::prefs::Prefs;
use crate::prompt::mage_home;
use crate::workflow::Workflow;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct File {
    pub default: Option<String>,
    #[serde(default)]
    pub last_endpoint: Option<String>,
    #[serde(default)]
    pub endpoint: BTreeMap<String, Endpoint>,
    #[serde(default)]
    pub search: Search,
    pub theme: Option<String>,
    pub splash: Option<bool>,
    pub animations: Option<bool>,
    pub fps: Option<u32>,
    pub reasoning: Option<ReasoningSetting>,
    pub tools: Option<String>,
    pub receipts: Option<String>,
    pub bus: Option<bool>,
    pub layout: Option<String>,
    pub ban_comments: Option<bool>,
    pub ban_tests: Option<bool>,
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
    pub modules: BTreeMap<String, bool>,
    #[serde(default)]
    pub mcp: BTreeMap<String, crate::mcp::Entry>,
    #[serde(default)]
    pub hooks: crate::hooks::Hooks,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum SeatRow {
    Name(String),
    Full {
        endpoint: String,
        model: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ReasoningSetting {
    Flag(bool),
    Text(String),
}

#[derive(Debug, Default, Deserialize)]
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

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Lsp {
    pub module: Option<String>,
    pub command: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize)]
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
    pub wire: Option<String>,
    #[serde(default)]
    pub fallback: Vec<String>,
    pub compact_tokens: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
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
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct Search {
    pub count: Option<usize>,
    pub allowed_domains: Option<Vec<String>>,
    pub excluded_domains: Option<Vec<String>>,
    pub backend: Option<String>,
    pub api_key: Option<String>,
    pub url: Option<String>,
}

pub(crate) fn path() -> Result<PathBuf> {
    Ok(mage_home()?.join("config.toml"))
}

pub(crate) fn load() -> Result<File> {
    load_from(&path()?)
}

pub fn sandbox_on() -> Result<bool> {
    Ok(load()?.sandbox.on)
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
    if file.theme.as_deref().is_some_and(|theme| {
        !matches!(
            theme,
            "space" | "fantasy" | "nord" | "gruvbox" | "solarized" | "light"
        )
    }) {
        return Err(Error::Config(
            "config.toml: `theme` must be space, fantasy, nord, gruvbox, solarized, or light"
                .to_string(),
        ));
    }
    if file
        .fps
        .is_some_and(|fps| !(crate::prefs::FPS_MIN..=crate::prefs::FPS_MAX).contains(&fps))
    {
        return Err(Error::Config(format!(
            "config.toml: `fps` must be a whole number from {} to {}",
            crate::prefs::FPS_MIN,
            crate::prefs::FPS_MAX
        )));
    }
    match file.secrets.as_deref().map(str::trim) {
        None | Some("") | Some("file") | Some("keychain") => {}
        Some(other) => {
            return Err(Error::Config(format!(
                "config.toml: secrets must be \"file\" or \"keychain\", not `{other}`"
            )));
        }
    }
    if file.lsp.module.is_some() && file.lsp.command.is_some() {
        return Err(Error::Config(
            "config.toml: [lsp] accepts `module` or `command`, not both".to_string(),
        ));
    }
    if let Some(module) = file.lsp.module.as_deref() {
        crate::language::named(module)?;
    }
    for name in file.modules.keys() {
        crate::modules::named(name)?;
    }
    if file.lsp.command.as_ref().is_some_and(Vec::is_empty) {
        return Err(Error::Config(
            "config.toml: [lsp] command must not be empty".to_string(),
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
    document["fps"] = value(i64::from(prefs.fps));
    document["reasoning"] = value(prefs.reasoning.name());
    document["receipts"] = value(prefs.receipts.name());
    document["tools"] = value(prefs.tools.name());
    document["bus"] = value(prefs.bus);
    document["ban_comments"] = value(prefs.ban_comments);
    document["ban_tests"] = value(prefs.ban_tests);
    document["layout"] = value(&prefs.layout);
    let mut hotkeys = toml_edit::InlineTable::new();
    for (action, binding) in prefs.hotkeys.entries() {
        hotkeys.insert(action.name(), binding.into());
    }
    document["hotkeys"] = toml_edit::Item::Value(toml_edit::Value::InlineTable(hotkeys));
    match &prefs.last_endpoint {
        Some(name) => document["last_endpoint"] = value(name),
        None => {
            document.as_table_mut().remove("last_endpoint");
        }
    }
    store::write(path, document.to_string())?;
    Ok(())
}

pub(crate) fn remove_key(path: &Path, name: &str) -> Result<bool> {
    let mut document = read_document(path)?;
    let removed = document["endpoint"][name]
        .as_table_like_mut()
        .is_some_and(|row| row.remove("api_key").is_some());
    if removed {
        store::write(path, document.to_string())?;
    }
    Ok(removed)
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
    store::write(path, document.to_string())?;
    Ok(())
}

pub(crate) fn save_module(path: &Path, name: &str, on: bool) -> Result<()> {
    let mut document = read_document(path)?;
    document["modules"][name] = value(on);
    if let Some(table) = document["modules"].as_table_mut() {
        table.set_implicit(true);
    }
    store::write(path, document.to_string())?;
    Ok(())
}

pub(crate) fn save_default_workflow(path: &Path, name: &str) -> Result<()> {
    let mut document = read_document(path)?;
    document["workflow"] = value(name);
    store::write(path, document.to_string())?;
    Ok(())
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

pub(crate) fn sandbox_entry(path: &Path) -> String {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    if let Some(home) = home
        && let Ok(rest) = path.strip_prefix(&home)
    {
        let rest = rest.to_string_lossy();
        if rest.is_empty() {
            return "~/".to_string();
        }
        return format!("~/{rest}");
    }
    path.display().to_string()
}

pub(crate) fn save_sandbox_write(path: &Path, dir: &str) -> Result<()> {
    let mut document = read_document(path)?;
    let entry = if Path::new(dir).is_absolute() {
        sandbox_entry(Path::new(dir))
    } else {
        dir.to_string()
    };
    let existing = document
        .get("sandbox")
        .and_then(|sandbox| sandbox.get("write"));
    if existing.is_some_and(|item| !item.is_none() && item.as_array().is_none()) {
        return Err(Error::Config(
            "config.toml: [sandbox] write must be an array".to_string(),
        ));
    }
    let mut array = existing
        .and_then(toml_edit::Item::as_array)
        .cloned()
        .unwrap_or_else(toml_edit::Array::new);
    if !array
        .iter()
        .any(|value| value.as_str() == Some(entry.as_str()))
    {
        array.push(entry.as_str());
    }
    document["sandbox"]["write"] = toml_edit::Item::Value(toml_edit::Value::Array(array));
    if let Some(table) = document["sandbox"].as_table_mut() {
        table.set_implicit(true);
    }
    store::write(path, document.to_string())?;
    Ok(())
}
