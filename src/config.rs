use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::RwLock;

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
    pub splash: Option<bool>,
    pub animations: Option<bool>,
    pub fps: Option<u32>,
    pub reasoning: Option<ReasoningSetting>,
    pub tools: Option<String>,
    pub diff_layout: Option<crate::prefs::DiffLayout>,
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
    pub import_mcp: Option<bool>,
    #[serde(default)]
    pub hooks: crate::hooks::Hooks,
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
    pub wire: Option<String>,
    #[serde(default)]
    pub fallback: Vec<String>,
    pub compact_tokens: Option<u32>,
    pub images: Option<bool>,
    pub keep_reasoning: Option<bool>,
    #[serde(flatten)]
    pub other: BTreeMap<String, toml::Value>,
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

static PARSED: RwLock<Option<File>> = RwLock::new(None);
static WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn file() -> Result<File> {
    if let Some(file) = PARSED.read().ok().and_then(|cached| cached.clone()) {
        return Ok(file);
    }
    let file = parse(&path()?)?;
    drop(
        PARSED
            .write()
            .map(|mut cached| *cached = Some(file.clone())),
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
    let bad = |field: &str| Error::Config(format!("{}: {field}", tilde(path)));
    let file: File = toml::from_str(&text).map_err(|error| {
        let line = error.span().map_or(1, |span| {
            text.get(..span.start)
                .map_or(1, |head| head.matches('\n').count() + 1)
        });
        bad(&format!(
            "line {line}: {} — fix that line, or move the file aside to start fresh",
            error.message().trim().replace('\n', "; ")
        ))
    })?;
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
            "summonr: {} has unknown key{} {} — check the spelling; summonr ignores {}",
            tilde(path),
            if unknown.len() == 1 { "" } else { "s" },
            unknown.join(", "),
            if unknown.len() == 1 { "it" } else { "them" }
        );
    }
    if file
        .tools
        .as_deref()
        .is_some_and(|tools| !matches!(tools, "compact" | "full"))
    {
        return Err(bad("`tools` must be `compact` or `full`"));
    }
    const THEMES: &[&str] = &["space", "fantasy", "nord", "gruvbox", "solarized", "light"];
    if file
        .theme
        .as_deref()
        .is_some_and(|theme| !THEMES.contains(&theme))
    {
        return Err(bad(
            "`theme` must be space, fantasy, nord, gruvbox, solarized, or light",
        ));
    }
    if file
        .fps
        .is_some_and(|fps| !(crate::prefs::FPS_MIN..=crate::prefs::FPS_MAX).contains(&fps))
    {
        return Err(bad(&format!(
            "`fps` must be a whole number from {} to {}",
            crate::prefs::FPS_MIN,
            crate::prefs::FPS_MAX
        )));
    }
    if let Some(other) = file.secrets.as_deref().map(str::trim)
        && !matches!(other, "" | "file" | "keychain")
    {
        return Err(bad(&format!(
            "secrets must be \"file\" or \"keychain\", not `{other}`"
        )));
    }
    if file.lsp.module.is_some() && file.lsp.command.is_some() {
        return Err(bad("[lsp] accepts `module` or `command`, not both"));
    }
    if let Some(module) = file.lsp.module.as_deref() {
        crate::language::named(module)?;
    }
    if file.lsp.command.as_ref().is_some_and(Vec::is_empty) {
        return Err(bad("[lsp] command must not be empty"));
    }
    Ok(file)
}

mod edit;
pub use edit::tilde;
pub(crate) use edit::{
    remove_key, sandbox_entry, save_default_seat, save_default_workflow, save_module, save_prefs,
    save_sandbox_write, save_workflow,
};

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
