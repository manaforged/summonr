use std::collections::BTreeMap;
use std::path::Path;

use crate::config::{self, Endpoint as FileEndpoint, File};
use crate::error::{Error, Result};

const GLM_CN: &str = "https://open.bigmodel.cn/api/coding/paas/v4";
const GLM_INTL: &str = "https://api.z.ai/api/coding/paas/v4";
const QWEN_LOCAL: &str = "http://127.0.0.1:8081/v1";
const KIMI: &str = "https://api.kimi.com/coding/v1";
const GROK: &str = "https://api.x.ai/v1";
const COPILOT: &str = "https://api.githubcopilot.com";
const GITHUB_DEVICE: &str = "https://github.com/login/device/code";
const GITHUB_TOKEN: &str = "https://github.com/login/oauth/access_token";
const COPILOT_CLIENT: &str = "Iv1.b507a08c87ecfe98";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub name: String,
    pub model: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub oauth: Option<crate::config::OAuth>,
    pub body: Option<serde_json::Value>,
    pub context_window: Option<u32>,
    pub env: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    pub wire: Wire,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Wire {
    #[default]
    Completions,
    Anthropic,
    Gemini,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthInfo {
    pub name: String,
    pub device_login: bool,
    pub configured: bool,
    pub setup: String,
}

impl AuthInfo {
    #[must_use]
    pub fn status(&self) -> String {
        let state = if self.device_login {
            "device authentication available"
        } else if self.configured {
            "API key configured"
        } else {
            "API key missing"
        };
        format!("{}: {state} — {}", self.name, self.setup)
    }
}

impl Wire {
    fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "anthropic" | "claude" => Self::Anthropic,
            "gemini" | "google" => Self::Gemini,
            _ => Self::Completions,
        }
    }
}

#[must_use]
pub fn is_named(token: &str) -> bool {
    builtin(token).is_some()
        || names()
            .ok()
            .is_some_and(|names| names.iter().any(|name| name == token))
}

pub fn names() -> Result<Vec<String>> {
    let mut names: Vec<String> = PRESETS
        .iter()
        .map(|preset| preset.name.to_string())
        .collect();
    for name in config::load()?.endpoint.keys() {
        if !names.iter().any(|have| have == name) {
            names.push(name.clone());
        }
    }
    Ok(names)
}

pub fn auth_info(name: &str) -> Result<AuthInfo> {
    let file = config::load()?;
    let process_key = named_env_key(name)
        .or_else(|| env_key(&["MAGE_API_KEY"]))
        .is_some();
    auth_info_from(&file, name, process_key)
}

pub(crate) fn save_key(name: &str, key: &str) -> Result<()> {
    if !names()?.iter().any(|candidate| candidate == name) {
        return Err(Error::Config(format!("unknown endpoint `{name}`")));
    }
    let key = key.trim();
    if key.is_empty() {
        return Err(Error::Config("API key cannot be empty".to_string()));
    }
    config::save_key(&config::path()?, name, key)
}

fn auth_info_from(file: &File, name: &str, process_key: bool) -> Result<AuthInfo> {
    let row = file.endpoint.get(name);
    let builtin = preset(name);
    if row.is_none() && builtin.is_none() {
        return Err(Error::Config(format!("unknown endpoint `{name}`")));
    }
    let device_login = row.and_then(|row| row.auth.as_ref()).is_some_and(|auth| {
        auth.grant.as_deref() == Some("device_code")
            || auth
                .device_url
                .as_deref()
                .is_some_and(|url| !url.is_empty())
    }) || builtin.is_some_and(|preset| preset.oauth.is_some());
    if device_login {
        return Ok(AuthInfo {
            name: name.to_string(),
            device_login: true,
            configured: true,
            setup: format!("run `/login {name}` for device authentication"),
        });
    }
    let config_key = row
        .and_then(|row| row.api_key.as_deref())
        .is_some_and(|key| !key.trim().is_empty());
    let default_key = builtin.and_then(|preset| preset.default_key).is_some();
    let setup = builtin
        .map(|preset| preset.missing)
        .filter(|text| !text.is_empty())
        .unwrap_or("put api_key in ~/.mage/config.toml")
        .to_string();
    Ok(AuthInfo {
        name: name.to_string(),
        device_login: false,
        configured: config_key || process_key || default_key,
        setup,
    })
}

pub(crate) fn ready() -> Vec<String> {
    names()
        .unwrap_or_default()
        .into_iter()
        .filter(|name| resolve(Some(name), None).is_ok())
        .collect()
}

pub(crate) fn next(current: &str) -> Option<String> {
    let names = ready();
    if names.len() < 2 {
        return None;
    }
    let index = names.iter().position(|name| name == current).unwrap_or(0);
    Some(names[(index + 1) % names.len()].clone())
}

pub(crate) fn catalog(wire: Wire) -> Vec<String> {
    let mut models = Vec::new();
    for row in PRESETS {
        if row.wire != wire {
            continue;
        }
        if !models.iter().any(|model| model == row.model) {
            models.push(row.model.to_string());
        }
        for (_, canon) in row.aliases {
            if !models.iter().any(|model| model == *canon) {
                models.push((*canon).to_string());
            }
        }
    }
    models
}

pub fn resolve(name: Option<&str>, model: Option<&str>) -> Result<Endpoint> {
    resolve_inner(name, model, true)
}

pub fn resolve_unauthed(name: Option<&str>, model: Option<&str>) -> Result<Endpoint> {
    resolve_inner(name, model, false)
}

fn resolve_inner(name: Option<&str>, model: Option<&str>, require_key: bool) -> Result<Endpoint> {
    if let Ok(base) = std::env::var("MAGE_BASE_URL")
        && !base.trim().is_empty()
        && name.is_none()
    {
        return Ok(Endpoint {
            name: "env".to_string(),
            base_url: Some(trim_slash(&base)),
            model: model
                .map(str::to_string)
                .or_else(|| std::env::var("MAGE_MODEL").ok())
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "glm-5.3".to_string()),
            api_key: env_key(&["MAGE_API_KEY"]).or_else(glm_key),
            oauth: None,
            body: None,
            context_window: None,
            env: BTreeMap::new(),
            headers: BTreeMap::new(),
            wire: Wire::Completions,
        });
    }

    let file = config::load()?;
    let chosen = name
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| std::env::var("MAGE_ENDPOINT").ok())
        .or_else(|| file.last_endpoint.clone())
        .or_else(|| file.default.clone());

    let model_flag = model
        .map(str::to_string)
        .or_else(|| std::env::var("MAGE_MODEL").ok())
        .filter(|value| !value.trim().is_empty());

    let chosen = chosen.unwrap_or_else(|| "glm".to_string());
    let mut endpoint = builtin(&chosen).or_else(|| from_file(&file, &chosen));

    if let Some(ref mut endpoint) = endpoint
        && let Some(row) = file.endpoint.get(&chosen)
    {
        overlay(endpoint, row);
    }

    let mut endpoint = endpoint.ok_or_else(|| {
        Error::Config(format!(
            "unknown endpoint `{chosen}` — try glm, kimi, grok, qwen, copilot, openai, openrouter, anthropic, gemini, or a name from ~/.mage/config.toml"
        ))
    })?;

    if let Some(model) = model_flag {
        endpoint.model = named_model(&endpoint.name, &model);
    } else if endpoint.model.is_empty() {
        return Err(Error::Config(format!(
            "endpoint `{}` needs --model (or model in config.toml)",
            endpoint.name
        )));
    } else {
        endpoint.model = named_model(&endpoint.name, &endpoint.model);
    }

    fill_key(endpoint, require_key)
}

fn from_file(file: &File, chosen: &str) -> Option<Endpoint> {
    let row = file.endpoint.get(chosen)?;
    Some(Endpoint {
        name: chosen.to_string(),
        model: row.model.clone().unwrap_or_default(),
        base_url: row.base_url.as_deref().map(trim_slash),
        api_key: row.api_key.clone().filter(|key| !key.trim().is_empty()),
        oauth: row.auth.clone(),
        body: row.body.as_ref().map(json),
        context_window: row.context_window,
        env: row.env.clone(),
        headers: row.headers.clone(),
        wire: row
            .wire
            .as_deref()
            .map(Wire::parse)
            .unwrap_or(Wire::Completions),
    })
}

fn overlay(endpoint: &mut Endpoint, row: &FileEndpoint) {
    if let Some(base) = &row.base_url {
        endpoint.base_url = Some(trim_slash(base));
    }
    if let Some(model) = &row.model {
        endpoint.model = model.clone();
    }
    if let Some(key) = &row.api_key
        && !key.trim().is_empty()
    {
        endpoint.api_key = Some(key.clone());
    }
    if let Some(auth) = &row.auth {
        endpoint.oauth = Some(auth.clone());
    }
    if let Some(body) = &row.body {
        endpoint.body = Some(json(body));
    }
    if let Some(window) = row.context_window {
        endpoint.context_window = Some(window);
    }
    for (key, value) in &row.env {
        endpoint.env.insert(key.clone(), value.clone());
    }
    for (key, value) in &row.headers {
        endpoint.headers.insert(key.clone(), value.clone());
    }
    if let Some(wire) = &row.wire {
        endpoint.wire = Wire::parse(wire);
    }
}

struct Preset {
    name: &'static str,
    base_url: &'static str,
    model: &'static str,
    env_keys: &'static [&'static str],
    default_key: Option<&'static str>,
    missing: &'static str,
    aliases: &'static [(&'static str, &'static str)],
    oauth: Option<fn() -> crate::config::OAuth>,
    headers: &'static [(&'static str, &'static str)],
    wire: Wire,
}

const PRESETS: &[Preset] = &[
    Preset {
        name: "glm",
        base_url: GLM_CN,
        model: "glm-5.3",
        env_keys: &[
            "ZAI_CODING_API_KEY",
            "GLM_CODING_API_KEY",
            "ZAI_API_KEY",
            "GLM_API_KEY",
        ],
        default_key: None,
        missing: "set ZAI_CODING_API_KEY or ZAI_API_KEY, or put api_key in ~/.mage/config.toml",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
    },
    Preset {
        name: "glm-intl",
        base_url: GLM_INTL,
        model: "glm-5.3",
        env_keys: &[
            "ZAI_CODING_API_KEY",
            "GLM_CODING_API_KEY",
            "ZAI_API_KEY",
            "GLM_API_KEY",
        ],
        default_key: None,
        missing: "set ZAI_CODING_API_KEY or ZAI_API_KEY, or put api_key in ~/.mage/config.toml",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
    },
    Preset {
        name: "qwen",
        base_url: QWEN_LOCAL,
        model: "qwen3.8-27b-ud",
        env_keys: &[],
        default_key: Some("local"),
        missing: "",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
    },
    Preset {
        name: "kimi",
        base_url: KIMI,
        model: "k3",
        env_keys: &["KIMI_CODING_API_KEY", "KIMI_API_KEY", "MOONSHOT_API_KEY"],
        default_key: None,
        missing: "set KIMI_CODING_API_KEY or KIMI_API_KEY, or put a token in ~/.kimi-code/credentials/kimi-code.json",
        aliases: &[
            ("kimi-k3", "k3"),
            ("k3", "k3"),
            ("kimi-for-coding", "k3"),
            ("kimi-code", "k3"),
            ("kimi-coding", "k3"),
        ],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
    },
    Preset {
        name: "grok",
        base_url: GROK,
        model: "grok-4.6",
        env_keys: &["XAI_API_KEY", "GROK_API_KEY"],
        default_key: None,
        missing: "set XAI_API_KEY, or put api_key in ~/.mage/config.toml",
        aliases: &[
            ("grok", "grok-4.6"),
            ("grok-4-6", "grok-4.6"),
            ("grok4.6", "grok-4.6"),
        ],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
    },
    Preset {
        name: "copilot",
        base_url: COPILOT,
        model: "gpt-4o",
        env_keys: &[],
        default_key: None,
        missing: "run `summon login copilot`",
        aliases: &[],
        oauth: Some(copilot_oauth),
        headers: &[
            ("Editor-Version", "vscode/1.95.3"),
            ("Editor-Plugin-Version", "copilot-chat/0.24.1"),
            ("Copilot-Integration-Id", "vscode-chat"),
        ],
        wire: Wire::Completions,
    },
    Preset {
        name: "openai",
        base_url: "https://api.openai.com/v1",
        model: "gpt-4o",
        env_keys: &["OPENAI_API_KEY"],
        default_key: None,
        missing: "set OPENAI_API_KEY, or put api_key in ~/.mage/config.toml",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
    },
    Preset {
        name: "openrouter",
        base_url: "https://openrouter.ai/api/v1",
        model: "openai/gpt-4o",
        env_keys: &["OPENROUTER_API_KEY", "OPEN_ROUTER_API_KEY"],
        default_key: None,
        missing: "set OPENROUTER_API_KEY, or put api_key in ~/.mage/config.toml",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
    },
    Preset {
        name: "anthropic",
        base_url: "https://api.anthropic.com/v1/",
        model: "claude-sonnet-4-5",
        env_keys: &["ANTHROPIC_API_KEY"],
        default_key: None,
        missing: "set ANTHROPIC_API_KEY, or put api_key in ~/.mage/config.toml",
        aliases: &[
            ("sonnet", "claude-sonnet-4-5"),
            ("claude", "claude-sonnet-4-5"),
        ],
        oauth: None,
        headers: &[("anthropic-version", "2023-06-01")],
        wire: Wire::Anthropic,
    },
    Preset {
        name: "gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta/",
        model: "gemini-2.5-flash",
        env_keys: &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        default_key: None,
        missing: "set GEMINI_API_KEY or GOOGLE_API_KEY, or put api_key in ~/.mage/config.toml",
        aliases: &[("flash", "gemini-2.5-flash"), ("pro", "gemini-2.5-pro")],
        oauth: None,
        headers: &[],
        wire: Wire::Gemini,
    },
];

fn copilot_oauth() -> crate::config::OAuth {
    crate::config::OAuth {
        token_url: GITHUB_TOKEN.into(),
        client_id: COPILOT_CLIENT.into(),
        client_secret: None,
        refresh_token: None,
        scope: Some("read:user".into()),
        grant: Some("device_code".into()),
        device_url: Some(GITHUB_DEVICE.into()),
        exchange_url: Some("https://api.github.com/copilot_internal/v2/token".into()),
    }
}

fn preset(name: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|preset| preset.name == name)
}

fn fill_key(mut endpoint: Endpoint, require_key: bool) -> Result<Endpoint> {
    if endpoint.oauth.is_some() || endpoint.api_key.is_some() {
        return Ok(endpoint);
    }
    if let Some(key) = named_env_key(&endpoint.name).or_else(|| env_key(&["MAGE_API_KEY"])) {
        endpoint.api_key = Some(key);
        return Ok(endpoint);
    }
    if let Some(row) = preset(&endpoint.name) {
        if let Some(key) = row.default_key {
            endpoint.api_key = Some(key.to_string());
            return Ok(endpoint);
        }
        if require_key && !row.missing.is_empty() {
            return Err(Error::Config(row.missing.to_string()));
        }
    }
    Ok(endpoint)
}

fn named_env_key(name: &str) -> Option<String> {
    let row = preset(name)?;
    env_key(row.env_keys).or_else(|| {
        if name == "kimi" {
            kimi_cli_token()
        } else {
            None
        }
    })
}

fn builtin(name: &str) -> Option<Endpoint> {
    let row = preset(name)?;
    Some(Endpoint {
        name: row.name.into(),
        base_url: Some(row.base_url.into()),
        model: row.model.into(),
        api_key: row.default_key.map(str::to_string),
        oauth: row.oauth.map(|make| make()),
        body: None,
        context_window: None,
        env: BTreeMap::new(),
        headers: row
            .headers
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect(),
        wire: row.wire,
    })
}

fn json(value: &toml::Value) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}

fn named_model(endpoint: &str, model: &str) -> String {
    let folded = model.trim().to_ascii_lowercase();
    if let Some(row) = preset(endpoint) {
        for (alias, canon) in row.aliases {
            if *alias == folded {
                return (*canon).to_string();
            }
        }
    }
    model.to_string()
}

fn kimi_cli_token() -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let path = Path::new(&home).join(".kimi-code/credentials/kimi-code.json");
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let expires = value
        .get("expires_at")
        .and_then(serde_json::Value::as_i64)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    if expires <= now + 60 {
        return None;
    }
    value
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.trim().is_empty())
        .map(str::to_string)
}

fn glm_key() -> Option<String> {
    named_env_key("glm")
}

fn env_key(names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    })
}

fn trim_slash(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

#[cfg(test)]
#[path = "endpoint_tests.rs"]
mod tests;
