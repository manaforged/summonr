mod head;
mod tail;

use super::*;
use crate::provider::Spec;

pub(super) struct Preset {
    pub(super) name: &'static str,
    pub(super) base_url: &'static str,
    pub(super) model: &'static str,
    pub(super) env_keys: &'static [&'static str],
    pub(super) default_key: Option<&'static str>,
    pub(super) missing: &'static str,
    pub(super) aliases: &'static [(&'static str, &'static str)],
    pub(super) oauth: Option<fn() -> crate::config::OAuth>,
    pub(super) headers: &'static [(&'static str, &'static str)],
    pub(super) wire: Wire,
    pub(super) reasoning: Spec,
    pub(super) context_window: Option<u32>,
    pub(super) opencode: bool,
    pub(super) prompt_cache_key: bool,
    pub(super) cli_token: Option<CliToken>,
    pub(super) opencode_oauth: bool,
    pub(super) env_default: bool,
    pub(super) plan: bool,
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

const OFF: &[&str] = &["claude-pro"];

pub(super) fn presets() -> impl Iterator<Item = &'static Preset> {
    HEAD.iter()
        .chain(TAIL)
        .filter(|preset| !OFF.contains(&preset.name))
}

pub(super) fn preset(name: &str) -> Option<&'static Preset> {
    presets().find(|preset| preset.name == name)
}

const PROVIDER_IDS: &[(&str, &[&str])] = &[
    ("zai", &["zai-coding-plan", "zai"]),
    ("zai-cn", &["zhipuai-coding-plan", "zhipuai"]),
    (
        "glm",
        &["zhipuai-coding-plan", "zhipuai", "zai-coding-plan", "zai"],
    ),
    ("glm-intl", &["zai-coding-plan", "zai"]),
    ("deepseek", &["deepseek"]),
    (
        "kimi",
        &[
            "kimi-code-plan-global",
            "kimi-code-plan-cn",
            "moonshotai",
            "moonshotai-cn",
        ],
    ),
    ("grok", &["xai"]),
    ("openai", &["openai"]),
    ("openrouter", &["openrouter"]),
    ("together", &["togetherai"]),
    ("groq", &["groq"]),
    ("cerebras", &["cerebras"]),
    ("mistral", &["mistral"]),
    ("anthropic", &["anthropic"]),
    ("claude-pro", &["anthropic"]),
    ("gemini", &["google"]),
    ("muse", &["meta"]),
    ("zen", &["opencode"]),
    ("go", &["opencode-go"]),
    ("opencode", &["opencode"]),
    ("fireworks", &["fireworks-ai"]),
    ("deepinfra", &["deepinfra"]),
    ("minimax", &["minimax-coding-plan", "minimax"]),
    ("minimax-cn", &["minimax-cn-coding-plan", "minimax-cn"]),
    ("copilot", &["github-copilot"]),
];

pub(crate) fn catalog_providers() -> impl Iterator<Item = &'static str> {
    PROVIDER_IDS.iter().flat_map(|(_, ids)| ids.iter().copied())
}

#[must_use]
pub(crate) fn provider_ids(name: &str) -> &'static [&'static str] {
    PROVIDER_IDS
        .iter()
        .find_map(|(row, ids)| (*row == name).then_some(*ids))
        .unwrap_or_default()
}

use head::HEAD;
use tail::TAIL;
