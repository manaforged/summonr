use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Off {
    Omit,
    #[serde(rename = "none")]
    EffortNone,
    #[serde(rename = "disabled")]
    ThinkingDisabled,
    BetweenTools,
    #[serde(rename = "chat_template_kwargs")]
    TemplateKwarg,
    EnableThinking,
    Unsupported,
}

impl Off {
    pub fn fragment(self) -> Option<(&'static str, Value)> {
        match self {
            Self::ThinkingDisabled => Some(("thinking", json!({"type": "disabled"}))),
            Self::BetweenTools => Some(("thinking", json!({"type": "between_tools"}))),
            Self::TemplateKwarg => {
                Some(("chat_template_kwargs", json!({"enable_thinking": false})))
            }
            Self::EnableThinking => Some(("enable_thinking", json!(false))),
            Self::Omit | Self::EffortNone | Self::Unsupported => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl<'de> Deserialize<'de> for Effort {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text)
            .ok_or_else(|| serde::de::Error::custom(format!("effort {}", Self::rejects(&text))))
    }
}

impl Effort {
    pub const ALL: [Self; 7] = [
        Self::None,
        Self::Minimal,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::XHigh,
        Self::Max,
    ];

    #[must_use]
    pub fn rejects(text: &str) -> String {
        let names: Vec<&str> = Self::ALL.iter().map(|effort| effort.as_str()).collect();
        let (last, rest) = names.split_last().unwrap_or((&"", &[]));
        format!("takes {}, or {last}, not `{text}`", rest.join(", "))
    }

    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "none" | "off" => Some(Self::None),
            "minimal" => Some(Self::Minimal),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "xhigh" => Some(Self::XHigh),
            "max" => Some(Self::Max),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        }
    }

    #[must_use]
    pub fn next(self) -> Self {
        match self {
            Self::None => Self::Minimal,
            Self::Minimal => Self::Low,
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High => Self::XHigh,
            Self::XHigh => Self::Max,
            Self::Max => Self::None,
        }
    }

    #[must_use]
    pub fn clamp(self, levels: &[Self]) -> Self {
        self.rounded(levels, false)
    }

    #[must_use]
    pub fn rounded(self, levels: &[Self], up: bool) -> Self {
        levels
            .iter()
            .copied()
            .min_by_key(|level| {
                let lean = if up {
                    u8::MAX - *level as u8
                } else {
                    *level as u8
                };
                ((*level as u8).abs_diff(self as u8), lean)
            })
            .unwrap_or(self)
    }

    #[must_use]
    pub fn shifted(self, levels: &[Self], delta: isize) -> Self {
        let at = levels.iter().position(|level| *level == self).unwrap_or(0);
        let next = (at as isize + delta).rem_euclid(levels.len() as isize);
        levels.get(next as usize).copied().unwrap_or(self)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    #[serde(default)]
    pub cached: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
}

impl Price {
    #[must_use]
    pub fn cost(&self, prompt: u64, completion: u64, cached: u64, written: u64) -> f64 {
        let cached = cached.min(prompt);
        let written = written.min(prompt - cached);
        let fresh = prompt - cached - written;
        (fresh as f64 * self.input
            + cached as f64 * self.cached
            + written as f64 * self.cache_write.unwrap_or(self.input)
            + completion as f64 * self.output)
            / 1_000_000.0
    }
}
