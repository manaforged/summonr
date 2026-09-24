use std::collections::BTreeMap;
use std::path::Path;

use crate::config::{self, Endpoint as FileEndpoint, File};
use crate::credentials::{flow, logged_in};
use crate::error::{Error, Result};
use crate::provider::{Effort, Price};
use crate::secret::{self, Source};

const GLM_CN: &str = "https://open.bigmodel.cn/api/coding/paas/v4";
const GLM_INTL: &str = "https://api.z.ai/api/coding/paas/v4";
const KIMI: &str = "https://api.kimi.com/coding/v1";
const GROK: &str = "https://api.x.ai/v1";
const MUSE: &str = "https://api.meta.ai/v1";
const ZEN: &str = "https://opencode.ai/zen/v1";
const GO: &str = "https://opencode.ai/zen/go/v1";
const COPILOT: &str = "https://api.githubcopilot.com";
const GITHUB_DEVICE: &str = "https://github.com/login/device/code";
const GITHUB_TOKEN: &str = "https://github.com/login/oauth/access_token";
const COPILOT_CLIENT: &str = "Iv1.b507a08c87ecfe98";
const CLAUDE_CLIENT: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const CLAUDE_SCOPES: &str = "org:create_api_key user:profile user:inference user:sessions:claude_code \
     user:mcp_servers user:file_upload";

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
    pub images: bool,
    pub keep_reasoning: bool,
    pub opencode: bool,
    pub prompt_cache_key: bool,
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
            Some(row) => {
                row.default_key.is_some() && row.env_keys.is_empty() && row.oauth.is_none()
            }
            None => {
                let url = self.base_url.as_deref();
                self.oauth.is_none()
                    && url
                        .and_then(hosted)
                        .is_none_or(|row| row.env_keys.is_empty())
                    && (self.name == "env" || url.is_some_and(local))
            }
        }
    }
}

fn local(url: &str) -> bool {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthInfo {
    pub name: String,
    pub flow: LoginFlow,
    pub configured: bool,
    pub source: Option<Source>,
    pub setup: String,
    pub env: Option<&'static str>,
    group: LoginGroup,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum LoginGroup {
    Key,
    Plan,
    Decide,
    Local,
}

impl LoginGroup {
    const fn heading(self) -> &'static str {
        match self {
            Self::Key => "API keys",
            Self::Plan => "Coding plans",
            Self::Decide => "Decisions",
            Self::Local => "Local",
        }
    }
}

impl AuthInfo {
    #[must_use]
    pub fn status(&self) -> String {
        format!("{}: {} — {}", self.name, self.state(), self.setup)
    }

    #[must_use]
    pub fn catalog_heading(&self) -> &'static str {
        self.group.heading()
    }

    #[must_use]
    pub fn keyless(&self) -> bool {
        self.group == LoginGroup::Local
    }

    #[must_use]
    pub fn catalog_rank(&self) -> u8 {
        self.group as u8
    }

    #[must_use]
    pub fn catalog_line(&self) -> String {
        format!("{:<14} {} — {}", self.name, self.state(), self.setup)
    }

    #[must_use]
    pub fn state(&self) -> String {
        match (self.flow.signs_in(), self.configured, self.source) {
            (true, true, _) | (false, true, None) => State::Ready.to_string(),
            (true, false, _) => State::SignIn.to_string(),
            (false, true, Some(Source::Builtin)) => State::NoKey.to_string(),
            (false, true, Some(source)) => format!("{} · {}", State::Ready, source.label()),
            (false, false, _) if self.keyless() => State::NoKey.to_string(),
            (false, false, _) => State::NeedsKey.to_string(),
        }
    }

    #[must_use]
    pub fn stored(&self) -> bool {
        self.configured
            && (self.flow.signs_in()
                || matches!(
                    self.source,
                    Some(Source::Keychain | Source::File | Source::Config)
                ))
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
    let mut names: Vec<String> = presets().map(|preset| preset.name.to_string()).collect();
    for name in config::file()?.endpoint.keys() {
        if !names.iter().any(|have| have == name) {
            names.push(name.clone());
        }
    }
    Ok(names)
}

fn login_group(name: &str, endpoint: &Endpoint) -> LoginGroup {
    if endpoint.wire == Wire::Decisions {
        return LoginGroup::Decide;
    }
    if let Some(preset) = preset(name) {
        if endpoint.keyless() {
            return LoginGroup::Local;
        }
        if preset.oauth.is_some()
            || preset
                .env_keys
                .iter()
                .any(|key| key.contains("CODING") || key.starts_with("OPENCODE_"))
            || preset.base_url.contains("/coding/")
            || preset.base_url.contains("opencode.ai")
        {
            return LoginGroup::Plan;
        }
        return LoginGroup::Key;
    }
    let url = endpoint.base_url.as_deref().unwrap_or("");
    if url.contains("/coding/") || url.contains("opencode.ai") || url.contains("githubcopilot") {
        LoginGroup::Plan
    } else if endpoint.keyless() {
        LoginGroup::Local
    } else {
        LoginGroup::Key
    }
}

mod auth;
mod keys;
mod preset;
mod resolve;

pub use auth::*;
pub(crate) use keys::peer_token;
use keys::*;
pub(crate) use preset::provider_ids;
use preset::{preset, presets};
pub use resolve::*;

#[cfg(test)]
#[path = "endpoint_tests.rs"]
mod tests;
