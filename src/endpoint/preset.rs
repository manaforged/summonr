use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;

use super::*;
use crate::provider::levels::STANDARD;
use crate::provider::{Effort, EffortField, Off, Scale, Spec};

pub(super) struct Preset {
    pub(super) name: String,
    pub(super) base_url: String,
    pub(super) models_url: Option<&'static str>,
    pub(super) model: String,
    pub(super) env_keys: &'static [&'static str],
    pub(super) default_key: Option<&'static str>,
    pub(super) missing: String,
    pub(super) aliases: &'static [(String, String)],
    pub(super) oauth: Option<fn() -> crate::config::OAuth>,
    pub(super) headers: &'static [(String, String)],
    pub(super) wire: Wire,
    pub(super) reasoning: Spec,
    pub(super) context_window: Option<u32>,
    pub(super) opencode: bool,
    pub(super) prompt_cache_key: bool,
    pub(super) cli_token: Option<CliToken>,
    pub(super) opencode_oauth: bool,
    pub(super) env_default: bool,
    pub(super) plan: bool,
    pub(super) off: bool,
    pub(super) opencode_ids: &'static [&'static str],
}

pub(super) struct CliToken {
    pub(super) label: &'static str,
    pub(super) read: fn() -> Option<String>,
}

pub(super) const KIMI_CLI: CliToken = CliToken {
    label: "kimi-code",
    read: super::keys::kimi_cli_token,
};

pub(super) fn env_default() -> Option<&'static Preset> {
    presets().find(|preset| preset.env_default)
}

fn claude_oauth() -> crate::config::OAuth {
    crate::config::OAuth {
        token_url: "https://platform.claude.com/v1/oauth/token".into(),
        client_id: CLAUDE_CLIENT.into(),
        label: Some("Claude Pro or Max".into()),
        authorize_url: Some("https://claude.ai/oauth/authorize".into()),
        redirect_uri: Some("https://platform.claude.com/oauth/code/callback".into()),
        scope: Some(CLAUDE_SCOPES.into()),
        grant: Some("authorization_code".into()),
        token_style: Some("json".into()),
        authorize_params: BTreeMap::from([("code".to_string(), "true".to_string())]),
        ..crate::config::OAuth::default()
    }
}

fn chatgpt_oauth() -> crate::config::OAuth {
    let mut authorize_params = BTreeMap::from([
        ("agent_name_hint".to_string(), "summonr".to_string()),
        ("resource".to_string(), OPENAI_API.to_string()),
    ]);
    if let Some(host) = host_id() {
        authorize_params.insert("ext_agent_host_id".to_string(), host);
    }
    crate::config::OAuth {
        token_url: CHATGPT_TOKEN.into(),
        client_id: CHATGPT_CLIENT.into(),
        label: Some("ChatGPT Plus or Pro".into()),
        authorize_url: Some(CHATGPT_AUTHORIZE.into()),
        redirect_uri: Some(CHATGPT_REDIRECT.into()),
        scope: Some(CHATGPT_SCOPES.into()),
        grant: Some("authorization_code".into()),
        authorize_params,
        token_params: BTreeMap::from([("resource".to_string(), OPENAI_API.to_string())]),
        required_scope: Some(CHATGPT_PLAN_SCOPE.into()),
        ..crate::config::OAuth::default()
    }
}

fn host_id() -> Option<String> {
    let path = crate::prompt::summonr_home().ok()?.join("host-id");
    if let Ok(found) = std::fs::read_to_string(&path)
        && !found.trim().is_empty()
    {
        return Some(found.trim().to_string());
    }
    let fresh = uuid::Uuid::now_v7().urn().to_string();
    std::fs::write(&path, &fresh).ok()?;
    Some(fresh)
}

fn supergrok_oauth() -> crate::config::OAuth {
    crate::config::OAuth {
        token_url: XAI_TOKEN.into(),
        client_id: XAI_CLIENT.into(),
        label: Some("SuperGrok or X Premium+".into()),
        scope: Some(XAI_SCOPES.into()),
        grant: Some("device_code".into()),
        device_url: Some(XAI_DEVICE.into()),
        ..crate::config::OAuth::default()
    }
}

fn copilot_oauth() -> crate::config::OAuth {
    crate::config::OAuth {
        token_url: GITHUB_TOKEN.into(),
        client_id: COPILOT_CLIENT.into(),
        label: Some("GitHub Copilot".into()),
        client_secret: None,
        refresh_token: None,
        scope: Some("read:user".into()),
        grant: Some("device_code".into()),
        device_url: Some(GITHUB_DEVICE.into()),
        exchange_url: Some("https://api.github.com/copilot_internal/v2/token".into()),
        ..crate::config::OAuth::default()
    }
}

fn login(name: &str) -> Option<fn() -> crate::config::OAuth> {
    [
        ("claude", claude_oauth as fn() -> crate::config::OAuth),
        ("chatgpt", chatgpt_oauth),
        ("supergrok", supergrok_oauth),
        ("copilot", copilot_oauth),
    ]
    .into_iter()
    .find(|(known, _)| *known == name)
    .map(|(_, make)| make)
}

fn cli_token(name: &str) -> Option<CliToken> {
    (name == "kimi-code").then_some(KIMI_CLI)
}

fn wire(value: &str) -> std::result::Result<Wire, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "completions" => Ok(Wire::Completions),
        "responses" => Ok(Wire::Responses),
        "anthropic" => Ok(Wire::Anthropic),
        "gemini" => Ok(Wire::Gemini),
        other => Err(format!("unknown wire `{other}`")),
    }
}

#[derive(Deserialize)]
struct File {
    preset: Vec<Row>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    name: String,
    base_url: String,
    #[serde(default)]
    models_url: Option<String>,
    #[serde(default)]
    model: String,
    #[serde(default)]
    env_keys: Vec<String>,
    #[serde(default)]
    default_key: Option<String>,
    #[serde(default)]
    missing: Option<String>,
    #[serde(default)]
    aliases: BTreeMap<String, String>,
    #[serde(default)]
    oauth: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    wire: Option<String>,
    #[serde(default)]
    reasoning: ReasoningRow,
    #[serde(default)]
    context_window: Option<u32>,
    #[serde(default)]
    opencode: bool,
    #[serde(default)]
    prompt_cache_key: bool,
    #[serde(default)]
    cli_token: Option<String>,
    #[serde(default = "yes")]
    opencode_oauth: bool,
    #[serde(default)]
    env_default: bool,
    #[serde(default)]
    plan: bool,
    #[serde(default)]
    off: bool,
    #[serde(default)]
    opencode_ids: Vec<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ReasoningRow {
    #[serde(default)]
    field: Option<String>,
    #[serde(default)]
    levels: Option<Vec<Effort>>,
    #[serde(default)]
    off: Option<Off>,
    #[serde(default)]
    host_only: bool,
}

fn yes() -> bool {
    true
}

pub(super) fn table(text: &str) -> std::result::Result<Vec<Preset>, String> {
    let file: File = toml::from_str(text).map_err(|error| error.to_string())?;
    let mut seen: Vec<String> = Vec::new();
    let mut rows = Vec::new();
    for row in file.preset {
        let name = row.name.trim().to_string();
        let base_url = row.base_url.trim().to_string();
        if name.is_empty() || base_url.is_empty() {
            return Err("every preset needs a name and a base_url".to_string());
        }
        if seen.contains(&name) {
            return Err(format!("preset `{name}` is listed twice"));
        }
        seen.push(name.clone());
        let oauth = match row.oauth.as_deref() {
            None => None,
            Some(named) => Some(
                login(named).ok_or_else(|| format!("`{name}` names unknown login `{named}`"))?,
            ),
        };
        let cli_token = match row.cli_token.as_deref() {
            None => None,
            Some(named) => Some(
                cli_token(named)
                    .ok_or_else(|| format!("`{name}` names unknown cli token `{named}`"))?,
            ),
        };
        let wire = match row.wire.as_deref() {
            None => Wire::Completions,
            Some(named) => {
                wire(named).map_err(|error| format!("`{name}` has an invalid wire: {error}"))?
            }
        };
        for key in &row.env_keys {
            if !key
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                || key.is_empty()
            {
                return Err(format!("`{name}` lists env key `{key}`"));
            }
        }
        let reasoning = {
            let field = match row.reasoning.field.as_deref() {
                None | Some("reasoning_effort") => EffortField::ReasoningEffort,
                Some("reasoning_object") => EffortField::ReasoningObject,
                Some(other) => {
                    return Err(format!("`{name}` names unknown effort field `{other}`"));
                }
            };
            let mut levels = row.reasoning.levels.unwrap_or_else(|| STANDARD.to_vec());
            levels.sort();
            levels.dedup();
            let off = row.reasoning.off.unwrap_or(Off::Omit);
            Spec {
                field,
                scale: Scale::new(Box::leak(levels.into_boxed_slice()), off),
                by_model: !row.reasoning.host_only,
            }
        };
        let missing = row.missing.clone().unwrap_or_else(|| {
            if row.env_keys.is_empty() {
                format!("run `summonr login {name}`")
            } else {
                format!("run `summonr login {name}` or set {}", row.env_keys[0])
            }
        });
        let leak = |values: &[String]| -> &'static [&'static str] {
            let kept: Vec<&'static str> = values
                .iter()
                .map(|value| Box::leak(value.clone().into_boxed_str()) as &'static str)
                .collect();
            Box::leak(kept.into_boxed_slice())
        };
        rows.push(Preset {
            name,
            base_url,
            models_url: row
                .models_url
                .as_deref()
                .map(|url| Box::leak(url.to_string().into_boxed_str()) as &'static str),
            model: row.model,
            env_keys: leak(&row.env_keys),
            default_key: row
                .default_key
                .as_deref()
                .map(|key| Box::leak(key.to_string().into_boxed_str()) as &'static str),
            missing,
            aliases: Box::leak(
                row.aliases
                    .into_iter()
                    .collect::<Vec<(String, String)>>()
                    .into_boxed_slice(),
            ),
            oauth,
            headers: Box::leak(
                row.headers
                    .into_iter()
                    .collect::<Vec<(String, String)>>()
                    .into_boxed_slice(),
            ),
            wire,
            reasoning,
            context_window: row.context_window,
            opencode: row.opencode,
            prompt_cache_key: row.prompt_cache_key,
            cli_token,
            opencode_oauth: row.opencode_oauth,
            env_default: row.env_default,
            plan: row.plan,
            off: row.off,
            opencode_ids: leak(&row.opencode_ids),
        });
    }
    Ok(rows)
}

const PRESETS: &str = include_str!("presets.toml");

pub(super) fn listed() -> &'static [Preset] {
    static ROWS: OnceLock<Vec<Preset>> = OnceLock::new();
    ROWS.get_or_init(|| match table(PRESETS) {
        Ok(rows) => rows,
        Err(error) => panic!("the built-in provider table is invalid: {error}"),
    })
}

pub(super) fn presets() -> impl Iterator<Item = &'static Preset> {
    listed().iter().filter(|preset| !preset.off)
}

pub(super) fn preset(name: &str) -> Option<&'static Preset> {
    presets().find(|preset| preset.name == name)
}

pub(crate) fn catalog_providers() -> impl Iterator<Item = &'static str> {
    listed()
        .iter()
        .filter(|preset| !preset.off)
        .flat_map(|preset| preset.opencode_ids.iter().copied())
}

#[must_use]
pub(crate) fn provider_ids(name: &str) -> &'static [&'static str] {
    listed()
        .iter()
        .find(|preset| preset.name == name && !preset.off)
        .map_or(&[], |preset| preset.opencode_ids)
}
