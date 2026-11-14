use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The wire protocol an endpoint speaks.
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

impl Wire {
    /// The wire protocol named by `value`, accepting common aliases.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value.trim().to_ascii_lowercase().as_str() {
            "completions" => Self::Completions,
            "responses" => Self::Responses,
            "anthropic" | "claude" => Self::Anthropic,
            "gemini" | "google" => Self::Gemini,
            "decisions" | "systemone" | "system-one" => Self::Decisions,
            "agent-sdk" => Self::AgentSdk,
            _ => return None,
        })
    }
}

/// How long a prompt cache entry lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum CacheTtl {
    #[serde(rename = "5m")]
    FiveMinutes,
    #[serde(rename = "1h")]
    OneHour,
}

/// The region of a provider's service.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Region {
    Intl,
    Cn,
}

impl Region {
    /// The region's display name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Intl => "international",
            Self::Cn => "China",
        }
    }
}

/// How an endpoint is paid for: an API key or a coding plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    Key,
    Plan,
}

impl Access {
    /// The access kind's display name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Key => "API key",
            Self::Plan => "coding plan",
        }
    }
}

/// The region and access an endpoint setup chose, with any other keys kept.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Choice {
    #[serde(default)]
    pub region: Option<Region>,
    #[serde(default)]
    pub access: Option<Access>,
    #[serde(flatten, default)]
    pub other: BTreeMap<String, toml::Value>,
}
