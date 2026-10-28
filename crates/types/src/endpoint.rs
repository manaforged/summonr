use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum CacheTtl {
    #[serde(rename = "5m")]
    FiveMinutes,
    #[serde(rename = "1h")]
    OneHour,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Region {
    Intl,
    Cn,
}

impl Region {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Intl => "international",
            Self::Cn => "China",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Access {
    Key,
    Plan,
}

impl Access {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Key => "API key",
            Self::Plan => "coding plan",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct Choice {
    #[serde(default)]
    pub region: Option<Region>,
    #[serde(default)]
    pub access: Option<Access>,
    #[serde(flatten, default)]
    pub other: BTreeMap<String, toml::Value>,
}
