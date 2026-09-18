use std::collections::BTreeMap;
use std::path::Path;

use crate::config::{self, Endpoint as FileEndpoint, File};
use crate::credentials::{is_device, logged_in};
use crate::error::{Error, Result};
use crate::provider::{Effort, Price};
use crate::secret::{self, Source};

const GLM_CN: &str = "https://open.bigmodel.cn/api/coding/paas/v4";
const GLM_INTL: &str = "https://api.z.ai/api/coding/paas/v4";
const QWEN_LOCAL: &str = "http://127.0.0.1:8081/v1";
const KIMI: &str = "https://api.kimi.com/coding/v1";
const GROK: &str = "https://api.x.ai/v1";
const MUSE: &str = "https://api.meta.ai/v1";
const ZEN: &str = "https://opencode.ai/zen/v1";
const GO: &str = "https://opencode.ai/zen/go/v1";
const COPILOT: &str = "https://api.githubcopilot.com";
const GITHUB_DEVICE: &str = "https://github.com/login/device/code";
const GITHUB_TOKEN: &str = "https://github.com/login/oauth/access_token";
const COPILOT_CLIENT: &str = "Iv1.b507a08c87ecfe98";

#[derive(Clone, Debug, PartialEq)]
pub struct Endpoint {
    pub name: String,
    pub model: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    pub source: Option<Source>,
    pub oauth: Option<crate::config::OAuth>,
    pub body: Option<serde_json::Value>,
    pub context_window: Option<u32>,
    pub idle_timeout_secs: Option<u64>,
    pub effort: Option<Effort>,
    pub price: Option<Price>,
    pub env: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    pub wire: Wire,
    pub fallback: Vec<String>,
    pub compact_tokens: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Wire {
    #[default]
    Completions,
    Responses,
    Anthropic,
    Gemini,
    Decisions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthInfo {
    pub name: String,
    pub device_login: bool,
    pub configured: bool,
    pub source: Option<Source>,
    pub setup: String,
}

impl AuthInfo {
    #[must_use]
    pub fn status(&self) -> String {
        format!("{}: {} — {}", self.name, self.state(), self.setup)
    }

    pub(crate) fn state(&self) -> String {
        match (self.device_login, self.configured, self.source) {
            (true, true, _) => "login configured".to_string(),
            (true, false, _) => "login required".to_string(),
            (false, true, Some(source)) => format!("key · {}", source.label()),
            (false, true, None) => "key configured".to_string(),
            (false, false, _) => "key missing".to_string(),
        }
    }
}

impl Wire {
    pub(crate) fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "responses" => Self::Responses,
            "anthropic" | "claude" => Self::Anthropic,
            "gemini" | "google" => Self::Gemini,
            "decisions" | "systemone" | "system-one" => Self::Decisions,
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
    let endpoint = resolve_inner(Some(name), None, false, false)?;
    let mut info = AuthInfo {
        name: name.to_string(),
        device_login: endpoint.oauth.as_ref().is_some_and(is_device),
        configured: endpoint.api_key.is_some(),
        source: endpoint.source,
        setup: preset(name)
            .map(|preset| preset.missing.replace("NAME", name))
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| format!("run `/login {name}` to configure an API key")),
    };
    if let Some(spec) = endpoint.oauth.as_ref().filter(|spec| is_device(spec)) {
        info.setup = format!("run `/login {name}` for device authentication");
        info.configured = match logged_in(spec, name) {
            Ok(ready) => ready,
            Err(error) => {
                info.setup = format!("{error}; {}", info.setup);
                false
            }
        };
    }
    Ok(info)
}

pub fn save_key(name: &str, key: &str) -> Result<Source> {
    if !names()?.iter().any(|candidate| candidate == name) {
        return Err(Error::Config(format!("unknown endpoint `{name}`")));
    }
    let key = key.trim();
    if key.is_empty() {
        return Err(Error::Config("API key cannot be empty".to_string()));
    }
    let source = secret::set(name, key)?;
    config::remove_key(&config::path()?, name)?;
    Ok(source)
}

pub fn forget_key(name: &str) -> Result<bool> {
    let stored = secret::forget(name)?;
    let token = secret::forget(&format!("oauth:{name}"))?;
    let legacy = crate::secret::forget(name)?;
    let config = config::remove_key(&config::path()?, name)?;
    Ok(stored || token || legacy || config)
}

pub fn adopt_keys() -> Result<Vec<String>> {
    adopt_from(&config::path()?, &secret::Store::discover()?)
}

pub(crate) fn adopt_from(path: &Path, store: &secret::Store) -> Result<Vec<String>> {
    let file = config::load_from(path)?;
    let mut moved = Vec::new();
    for (name, row) in &file.endpoint {
        let Some(key) = row
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
        else {
            continue;
        };
        if store.get(name)?.is_none() {
            store.set(name, key)?;
        }
        config::remove_key(path, name)?;
        moved.push(name.clone());
    }
    Ok(moved)
}

pub(crate) fn ready(require_model: bool) -> (Vec<Endpoint>, Vec<String>) {
    let mut names = match names() {
        Ok(names) => names,
        Err(error) => return (Vec::new(), vec![error.to_string()]),
    };
    if let Ok(endpoint) = resolve_inner(None, None, true, require_model)
        && !names.contains(&endpoint.name)
    {
        names.insert(0, endpoint.name);
    }
    let mut endpoints = Vec::new();
    let mut errors = Vec::new();
    for endpoint in names
        .into_iter()
        .filter_map(|name| resolve_inner(Some(&name), None, true, require_model).ok())
        .filter(|endpoint| endpoint.wire != Wire::Decisions)
    {
        let available = endpoint
            .oauth
            .as_ref()
            .map_or(Ok(true), |spec| logged_in(spec, &endpoint.name));
        match available {
            Ok(true) => endpoints.push(endpoint),
            Ok(false) => {}
            Err(error) => errors.push(format!("{}: {error}", endpoint.name)),
        }
    }
    (endpoints, errors)
}

pub fn decisions_keys() -> Vec<String> {
    let mut names: Vec<String> = PRESETS
        .iter()
        .filter(|row| row.wire == Wire::Decisions)
        .flat_map(|row| row.env_keys.iter().map(|key| (*key).to_string()))
        .collect();
    names.dedup();
    names
}

pub fn first_decisions() -> Option<String> {
    let configured = names().unwrap_or_default();
    let shipped = PRESETS
        .iter()
        .filter(|row| row.wire == Wire::Decisions)
        .map(|row| row.name.to_string());
    configured.into_iter().chain(shipped).find(|name| {
        resolve_inner(Some(name), None, true, false)
            .is_ok_and(|endpoint| endpoint.wire == Wire::Decisions)
    })
}

pub(crate) fn step(current: &str, forward: bool) -> Option<String> {
    let (endpoints, _) = ready(true);
    if endpoints.is_empty() {
        return None;
    }
    let index = endpoints
        .iter()
        .position(|endpoint| endpoint.name == current);
    let target = match (index, forward) {
        (Some(index), true) => (index + 1) % endpoints.len(),
        (Some(index), false) => (index + endpoints.len() - 1) % endpoints.len(),
        (None, true) => 0,
        (None, false) => endpoints.len() - 1,
    };
    (endpoints[target].name != current).then(|| endpoints[target].name.clone())
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

pub fn fallbacks<P>(
    endpoint: &Endpoint,
    mut connect: impl FnMut(&Endpoint) -> Result<P>,
) -> (Vec<(P, String)>, Vec<String>) {
    let mut ready = Vec::new();
    let mut skipped = Vec::new();
    for name in &endpoint.fallback {
        match resolve(Some(name), None)
            .and_then(|fallback| Ok((connect(&fallback)?, fallback.model)))
        {
            Ok(pair) => ready.push(pair),
            Err(error) => skipped.push(format!("fallback {name} skipped: {error}")),
        }
    }
    (ready, skipped)
}

pub fn resolve(name: Option<&str>, model: Option<&str>) -> Result<Endpoint> {
    resolve_inner(name, model, true, true)
}

pub fn resolve_unauthed(name: Option<&str>, model: Option<&str>) -> Result<Endpoint> {
    resolve_inner(name, model, false, true)
}

fn resolve_inner(
    name: Option<&str>,
    model: Option<&str>,
    require_key: bool,
    require_model: bool,
) -> Result<Endpoint> {
    if let Ok(base) = std::env::var("MAGE_BASE_URL")
        && !base.trim().is_empty()
        && matches!(name, None | Some("env"))
    {
        return Ok(Endpoint {
            name: "env".to_string(),
            base_url: Some(trim_slash(&base)),
            model: model
                .map(str::to_string)
                .or_else(|| std::env::var("MAGE_MODEL").ok())
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "glm-5.3".to_string()),
            api_key: env_key(&["MAGE_API_KEY"]).or_else(|| named_env_key("glm-intl")),
            source: None,
            oauth: None,
            body: None,
            context_window: None,
            idle_timeout_secs: None,
            effort: None,
            price: None,
            env: BTreeMap::new(),
            headers: BTreeMap::new(),
            wire: Wire::Completions,
            fallback: Vec::new(),
            compact_tokens: None,
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

    let fallback = chosen.is_none();
    let chosen = chosen.unwrap_or_else(|| "zai".to_string());
    let mut endpoint = builtin(&chosen).or_else(|| from_file(&file, &chosen));

    if let Some(ref mut endpoint) = endpoint
        && let Some(row) = file.endpoint.get(&chosen)
    {
        overlay(endpoint, row);
    }

    let mut endpoint = endpoint.ok_or_else(|| {
        Error::Config(format!(
            "unknown endpoint `{chosen}` — try `summonr endpoints`, or a name from ~/.mage/config.toml"
        ))
    })?;

    if let Some(model) = model_flag {
        endpoint.model = named_model(&endpoint.name, &model);
    } else if require_model && endpoint.model.is_empty() {
        return Err(Error::Config(format!(
            "endpoint `{}` needs --model (or model in config.toml)",
            endpoint.name
        )));
    } else {
        endpoint.model = named_model(&endpoint.name, &endpoint.model);
    }

    fill_key(endpoint, require_key).map_err(|error| {
        if !fallback {
            return error;
        }
        let list = PRESETS
            .iter()
            .map(|preset| preset.name)
            .collect::<Vec<_>>()
            .join(", ");
        Error::Config(format!(
            "no endpoint is configured. Builtins: {list}. Set that host's key in the environment or api_key in ~/.mage/config.toml, run `summon login NAME`, or add an [endpoint.NAME] row; `summon endpoints` shows the status of each. (zai: {error})"
        ))
    })
}

fn from_file(file: &File, chosen: &str) -> Option<Endpoint> {
    let row = file.endpoint.get(chosen)?;
    Some(Endpoint {
        name: chosen.to_string(),
        model: row.model.clone().unwrap_or_default(),
        base_url: row.base_url.as_deref().map(trim_slash),
        api_key: row.api_key.clone().filter(|key| !key.trim().is_empty()),
        source: None,
        oauth: row.auth.clone(),
        body: row.body.as_ref().map(json),
        context_window: row.context_window,
        idle_timeout_secs: row.idle_timeout_secs,
        effort: row.effort,
        price: row.price,
        env: row.env.clone(),
        headers: row.headers.clone(),
        fallback: row.fallback.clone(),
        compact_tokens: row.compact_tokens,
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
    if let Some(effort) = row.effort {
        endpoint.effort = Some(effort);
    }
    if let Some(price) = row.price {
        endpoint.price = Some(price);
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
    if !row.fallback.is_empty() {
        endpoint.fallback.clone_from(&row.fallback);
    }
    if let Some(tokens) = row.compact_tokens {
        endpoint.compact_tokens = Some(tokens);
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
    context_window: Option<u32>,
}

const PRESETS: &[Preset] = &[
    Preset {
        name: "zai",
        base_url: GLM_INTL,
        model: "glm-5.3-flash",
        env_keys: &["ZAI_API_KEY", "ZAI_CODING_API_KEY", "ZHIPU_API_KEY"],
        default_key: None,
        missing: "set ZAI_API_KEY, or `summon login NAME`",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
        context_window: None,
    },
    Preset {
        name: "zai-cn",
        base_url: GLM_CN,
        model: "glm-5.3-flash",
        env_keys: &["ZAI_CODING_CN_API_KEY", "GLM_CODING_API_KEY", "GLM_API_KEY"],
        default_key: None,
        missing: "set ZAI_CODING_CN_API_KEY, or `summon login NAME`",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
        context_window: None,
    },
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
        missing: "set ZAI_CODING_API_KEY or ZAI_API_KEY, or `summon login NAME`",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
        context_window: None,
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
        missing: "set ZAI_CODING_API_KEY or ZAI_API_KEY, or `summon login NAME`",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
        context_window: None,
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
        context_window: None,
    },
    Preset {
        name: "kimi",
        base_url: KIMI,
        model: "k3",
        env_keys: &["KIMI_CODING_API_KEY", "KIMI_API_KEY", "MOONSHOT_API_KEY"],
        default_key: None,
        missing: "set KIMI_CODING_API_KEY or KIMI_API_KEY, put a token in ~/.kimi-code/credentials/kimi-code.json, or `summon login NAME`",
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
        context_window: None,
    },
    Preset {
        name: "grok",
        base_url: GROK,
        model: "grok-4.6",
        env_keys: &["XAI_API_KEY", "GROK_API_KEY"],
        default_key: None,
        missing: "set XAI_API_KEY, or `summon login NAME`",
        aliases: &[
            ("grok", "grok-4.6"),
            ("grok-4-6", "grok-4.6"),
            ("grok4.6", "grok-4.6"),
        ],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
        context_window: None,
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
        context_window: None,
    },
    Preset {
        name: "openai",
        base_url: "https://api.openai.com/v1",
        model: "gpt-4o",
        env_keys: &["OPENAI_API_KEY"],
        default_key: None,
        missing: "set OPENAI_API_KEY, or `summon login NAME`",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
        context_window: None,
    },
    Preset {
        name: "openrouter",
        base_url: "https://openrouter.ai/api/v1",
        model: "openai/gpt-4o",
        env_keys: &["OPENROUTER_API_KEY", "OPEN_ROUTER_API_KEY"],
        default_key: None,
        missing: "set OPENROUTER_API_KEY, or `summon login NAME`",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
        context_window: None,
    },
    Preset {
        name: "jev",
        base_url: "https://openrouter.ai/api/alpha/decisions",
        model: "~typesafe/jev-latest",
        env_keys: &["OPENROUTER_API_KEY", "OPEN_ROUTER_API_KEY"],
        default_key: None,
        missing: "set OPENROUTER_API_KEY, or `summon login jev`",
        aliases: &[
            ("jev-latest", "~typesafe/jev-latest"),
            ("jev", "~typesafe/jev-latest"),
        ],
        oauth: None,
        headers: &[],
        wire: Wire::Decisions,
        context_window: Some(32_000),
    },
    Preset {
        name: "typesafe",
        base_url: "https://api.typesafe.ai/v1/systemone",
        model: "jev-latest",
        env_keys: &["TYPESAFE_API_KEY"],
        default_key: None,
        missing: "set TYPESAFE_API_KEY, or `summon login typesafe`",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Decisions,
        context_window: Some(32_000),
    },
    Preset {
        name: "together",
        base_url: "https://api.together.ai/v1",
        model: "",
        env_keys: &["TOGETHER_API_KEY"],
        default_key: None,
        missing: "set TOGETHER_API_KEY, or `summon login NAME`",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
        context_window: None,
    },
    Preset {
        name: "groq",
        base_url: "https://api.groq.com/openai/v1",
        model: "",
        env_keys: &["GROQ_API_KEY"],
        default_key: None,
        missing: "set GROQ_API_KEY, or `summon login NAME`",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
        context_window: None,
    },
    Preset {
        name: "cerebras",
        base_url: "https://api.cerebras.ai/v1",
        model: "",
        env_keys: &["CEREBRAS_API_KEY"],
        default_key: None,
        missing: "set CEREBRAS_API_KEY, or `summon login NAME`",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
        context_window: None,
    },
    Preset {
        name: "mistral",
        base_url: "https://api.mistral.ai/v1",
        model: "",
        env_keys: &["MISTRAL_API_KEY"],
        default_key: None,
        missing: "set MISTRAL_API_KEY, or `summon login NAME`",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
        context_window: None,
    },
    Preset {
        name: "anthropic",
        base_url: "https://api.anthropic.com/v1/",
        model: "claude-sonnet-4-5",
        env_keys: &["ANTHROPIC_API_KEY"],
        default_key: None,
        missing: "set ANTHROPIC_API_KEY, or `summon login NAME`",
        aliases: &[
            ("sonnet", "claude-sonnet-4-5"),
            ("claude", "claude-sonnet-4-5"),
        ],
        oauth: None,
        headers: &[("anthropic-version", "2023-06-01")],
        wire: Wire::Anthropic,
        context_window: None,
    },
    Preset {
        name: "gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta/",
        model: "gemini-2.5-flash",
        env_keys: &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        default_key: None,
        missing: "set GEMINI_API_KEY or GOOGLE_API_KEY, or `summon login NAME`",
        aliases: &[("flash", "gemini-2.5-flash"), ("pro", "gemini-2.5-pro")],
        oauth: None,
        headers: &[],
        wire: Wire::Gemini,
        context_window: None,
    },
    Preset {
        name: "muse",
        base_url: MUSE,
        model: "muse-spark-1.3",
        env_keys: &["META_API_KEY", "MODEL_API_KEY"],
        default_key: None,
        missing: "set META_API_KEY, or `summon login NAME`",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
        context_window: None,
    },
    Preset {
        name: "zen",
        base_url: ZEN,
        model: "glm-5.3-flash",
        env_keys: &["OPENCODE_API_KEY"],
        default_key: None,
        missing: "set OPENCODE_API_KEY, or `summon login zen`",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
        context_window: None,
    },
    Preset {
        name: "go",
        base_url: GO,
        model: "glm-5.3-flash",
        env_keys: &["OPENCODE_GO_API_KEY", "OPENCODE_API_KEY"],
        default_key: None,
        missing: "set OPENCODE_GO_API_KEY, or `summon login go`",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
        context_window: None,
    },
    Preset {
        name: "opencode",
        base_url: ZEN,
        model: "glm-5.3-flash",
        env_keys: &["OPENCODE_API_KEY"],
        default_key: None,
        missing: "set OPENCODE_API_KEY, or `summon login opencode`",
        aliases: &[],
        oauth: None,
        headers: &[],
        wire: Wire::Completions,
        context_window: None,
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
    if endpoint.oauth.is_some() {
        return Ok(endpoint);
    }
    if let Some(key) = named_env_key(&endpoint.name) {
        endpoint.api_key = Some(key);
        endpoint.source = Some(Source::Env);
        return Ok(endpoint);
    }
    if let Some((key, source)) = secret::get(&endpoint.name)? {
        endpoint.api_key = Some(key);
        endpoint.source = Some(source);
        return Ok(endpoint);
    }
    if endpoint.api_key.is_some() {
        endpoint.source = Some(Source::Config);
        return Ok(endpoint);
    }
    if let Some(row) = preset(&endpoint.name) {
        if let Some(key) = row.default_key {
            endpoint.api_key = Some(key.to_string());
            return Ok(endpoint);
        }
        if require_key && !row.missing.is_empty() {
            return Err(Error::Config(row.missing.replace("NAME", &endpoint.name)));
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
        source: None,
        oauth: row.oauth.map(|make| make()),
        body: None,
        context_window: row.context_window,
        idle_timeout_secs: None,
        effort: None,
        price: None,
        env: BTreeMap::new(),
        headers: row
            .headers
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect(),
        wire: row.wire,
        fallback: Vec::new(),
        compact_tokens: None,
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
    let now = i64::try_from(crate::bus::now()).ok()?;
    if expires <= now + 60 {
        return None;
    }
    value
        .get("access_token")
        .and_then(serde_json::Value::as_str)
        .filter(|token| !token.trim().is_empty())
        .map(str::to_string)
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
