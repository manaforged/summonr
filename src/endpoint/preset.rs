mod head;
mod tail;

use super::*;

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
    pub(super) context_window: Option<u32>,
    pub(super) opencode: bool,
    pub(super) prompt_cache_key: bool,
}

fn claude_oauth() -> crate::config::OAuth {
    crate::config::OAuth {
        token_url: "https://platform.claude.com/v1/oauth/token".into(),
        client_id: CLAUDE_CLIENT.into(),
        authorize_url: Some("https://claude.ai/oauth/authorize".into()),
        redirect_uri: Some("https://platform.claude.com/oauth/code/callback".into()),
        scope: Some(CLAUDE_SCOPES.into()),
        grant: Some("authorization_code".into()),
        token_style: Some("json".into()),
        authorize_params: BTreeMap::from([("code".to_string(), "true".to_string())]),
        ..crate::config::OAuth::default()
    }
}

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
        ..crate::config::OAuth::default()
    }
}

pub(super) fn presets() -> impl Iterator<Item = &'static Preset> {
    HEAD.iter().chain(TAIL)
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

#[must_use]
pub(crate) fn provider_ids(name: &str) -> &'static [&'static str] {
    PROVIDER_IDS
        .iter()
        .find_map(|(row, ids)| (*row == name).then_some(*ids))
        .unwrap_or_default()
}

use head::HEAD;
use tail::TAIL;
