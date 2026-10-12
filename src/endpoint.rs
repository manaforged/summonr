use std::collections::BTreeMap;

use crate::config::{self, Endpoint as FileEndpoint, File};
use crate::credentials::{flow, logged_in};
use crate::error::{Error, Result};
use crate::provider::{Effort, Price, Reasoning};
use crate::secret::{self, Source};

pub(crate) const ENV_ENDPOINT: &str = "env";
const PLAN_PATH_MARKER: &str = "/coding/";
const GITHUB_DEVICE: &str = "https://github.com/login/device/code";
const GITHUB_TOKEN: &str = "https://github.com/login/oauth/access_token";
const COPILOT_CLIENT: &str = "Iv1.b507a08c87ecfe98";
const CLAUDE_CLIENT: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const CLAUDE_SCOPES: &str = "org:create_api_key user:profile user:inference user:sessions:claude_code \
     user:mcp_servers user:file_upload";
const OPENAI_API: &str = "https://api.openai.com/v1";
const CHATGPT_AUTHORIZE: &str = "https://auth.openai.com/api/accounts/authorize";
const CHATGPT_TOKEN: &str = "https://auth.openai.com/api/accounts/oauth/token";
const CHATGPT_CLIENT: &str = "dynamic_agent_client";
const CHATGPT_REDIRECT: &str = "http://127.0.0.1:0/auth/callback";
const CHATGPT_PLAN_SCOPE: &str = "chatgpt.tokens.use.direct";
const CHATGPT_SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const XAI_DEVICE: &str = "https://auth.x.ai/oauth2/device/code";
const XAI_TOKEN: &str = "https://auth.x.ai/oauth2/token";
const XAI_CLIENT: &str = "b1a00492-073a-47ea-816f-4c329264a828";
const XAI_SCOPES: &str = "openid profile email offline_access grok-cli:access api:access";

#[derive(Clone, PartialEq)]
pub struct Endpoint {
    pub name: String,
    pub model: String,
    pub base_url: Option<String>,
    pub models_url: Option<String>,
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
    pub images: Option<bool>,
    pub keep_reasoning: bool,
    pub opencode: bool,
    pub prompt_cache_key: bool,
    pub assistant_after_tool: bool,
    pub cache_ttl: CacheTtl,
    pub cache_header: Option<String>,
    pub cache_marks: bool,
    pub(crate) reasoning: Reasoning,
}

impl std::fmt::Debug for Endpoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Endpoint")
            .field("name", &self.name)
            .field("model", &self.model)
            .field("base_url", &self.base_url)
            .field("models_url", &self.models_url)
            .field("api_key", &self.api_key.is_some())
            .field("source", &self.source)
            .field("oauth", &self.oauth.is_some())
            .field("body", &self.body)
            .field("context_window", &self.context_window)
            .field("idle_timeout_secs", &self.idle_timeout_secs)
            .field("effort", &self.effort)
            .field("price", &self.price)
            .field("env", &self.env.keys().collect::<Vec<_>>())
            .field("headers", &self.headers.keys().collect::<Vec<_>>())
            .field("wire", &self.wire)
            .field("fallback", &self.fallback)
            .field("compact_tokens", &self.compact_tokens)
            .field("images", &self.images)
            .field("keep_reasoning", &self.keep_reasoning)
            .field("opencode", &self.opencode)
            .field("prompt_cache_key", &self.prompt_cache_key)
            .field("assistant_after_tool", &self.assistant_after_tool)
            .field("cache_ttl", &self.cache_ttl)
            .field("cache_header", &self.cache_header)
            .field("reasoning", &self.reasoning)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Wire {
    #[default]
    Completions,
    Responses,
    Anthropic,
    Gemini,
    Decisions,
    AgentSdk,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize)]
pub enum CacheTtl {
    #[serde(rename = "5m")]
    FiveMinutes,
    #[serde(rename = "1h")]
    OneHour,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LoginFlow {
    #[default]
    Key,
    Device,
    Browser,
}

impl Endpoint {
    #[must_use]
    pub fn keyless(&self) -> bool {
        match preset(&self.name) {
            Some(row) => row.keyless(),
            None => {
                let url = self.base_url.as_deref();
                self.oauth.is_none()
                    && url
                        .and_then(hosted)
                        .is_none_or(|row| row.env_keys.is_empty())
                    && (self.name == ENV_ENDPOINT || url.is_some_and(local))
            }
        }
    }
}

pub(crate) fn local(url: &str) -> bool {
    match url::Url::parse(url).ok().as_ref().and_then(url::Url::host) {
        Some(url::Host::Ipv4(ip)) => {
            let [first, second, ..] = ip.octets();
            ip.is_loopback() || ip.is_private() || (first == 100 && second & 0xc0 == 64)
        }
        Some(url::Host::Ipv6(ip)) => ip.is_loopback() || ip.segments()[0] & 0xfe00 == 0xfc00,
        Some(url::Host::Domain(host)) => {
            host == "localhost" || host == "host.docker.internal" || host.ends_with(".local")
        }
        None => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Ready,
    SignIn,
    NoKey,
    NeedsKey,
    NeedsModel,
    KeyRejected,
    RateLimited,
    Offline,
    Error,
    Checking,
    Loading,
}

impl State {
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::SignIn => "sign in",
            Self::NoKey => "no key needed",
            Self::NeedsKey => "needs key",
            Self::NeedsModel => "needs model",
            Self::KeyRejected => "key rejected",
            Self::RateLimited => "rate limited",
            Self::Offline => "offline",
            Self::Error => "error",
            Self::Checking => "checking",
            Self::Loading => "loading",
        }
    }

    #[must_use]
    pub const fn failed(self) -> bool {
        matches!(
            self,
            Self::KeyRejected | Self::RateLimited | Self::Offline | Self::Error
        )
    }

    #[must_use]
    pub const fn setup(self) -> bool {
        matches!(self, Self::SignIn | Self::NeedsKey)
    }

    #[must_use]
    pub(crate) const fn blocked(self) -> bool {
        matches!(
            self,
            Self::SignIn
                | Self::NeedsKey
                | Self::NeedsModel
                | Self::KeyRejected
                | Self::Offline
                | Self::Error
        )
    }
}

impl std::fmt::Display for State {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.word())
    }
}

impl LoginFlow {
    #[must_use]
    pub fn signs_in(self) -> bool {
        !matches!(self, Self::Key)
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
    let file = config::file()?;
    Ok(route::provider_names(&file))
}

fn login_group(name: &str, endpoint: &Endpoint) -> LoginGroup {
    if endpoint.wire == Wire::Decisions {
        return LoginGroup::Decide;
    }
    if let Some(preset) = preset(name) {
        if endpoint.keyless() {
            return LoginGroup::Local;
        }
        if preset.plan {
            return LoginGroup::Plan;
        }
        return LoginGroup::Key;
    }
    let url = endpoint.base_url.as_deref().unwrap_or("");
    if url.contains(PLAN_PATH_MARKER) || hosted(url).is_some_and(|preset| preset.plan) {
        LoginGroup::Plan
    } else if endpoint.keyless() {
        LoginGroup::Local
    } else {
        LoginGroup::Key
    }
}

mod auth;
mod choice;
mod info;
mod keys;
mod preset;
mod reasoning;
mod resolve;
mod route;
mod server;

#[cfg(test)]
pub(crate) use auth::catalog;
pub use auth::{adopt_keys, auth_info, check_key, first_classify, login_seat, logout};
pub(crate) use auth::{ready, step};
pub use choice::{Need, Pick};
pub use info::AuthInfo;
use info::LoginGroup;
pub(crate) use keys::peer_token;
use keys::*;
pub use preset::label;
use preset::preset;
pub(crate) use preset::{catalog_providers, provider_ids, start_command};
pub(crate) use resolve::default_seat;
pub use resolve::{Near, chosen_by, closest, fallbacks, ordered_names, resolve, set_up, vision};
use resolve::{hosted, seat};
pub(crate) use route::{Choice, Region, Route, Toggle, remembered, select, toggle};
pub use server::{add_server, remove_server, set_default};

#[cfg(test)]
#[path = "endpoint_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "endpoint/effort_tests.rs"]
mod effort_tests;
