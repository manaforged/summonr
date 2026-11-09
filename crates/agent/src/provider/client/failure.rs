use std::collections::HashMap;
use std::sync::LazyLock;

use super::*;

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Class {
    Authentication,
    UsageLimit,
    Server,
    Http,
}

impl Class {
    fn kind(self) -> ProviderFailureKind {
        match self {
            Self::Authentication => ProviderFailureKind::Authentication,
            Self::UsageLimit => ProviderFailureKind::UsageLimit,
            Self::Server => ProviderFailureKind::Server,
            Self::Http => ProviderFailureKind::Http,
        }
    }
}

#[derive(Deserialize)]
struct Named {
    marker: String,
    kind: Class,
}

#[derive(Deserialize)]
struct Status {
    marker: String,
    code: u16,
}

#[derive(Deserialize)]
struct Words {
    all: Vec<String>,
    #[serde(default)]
    any: Vec<String>,
}

impl Words {
    fn matches(&self, lower: &str) -> bool {
        // complexity: bounded by the few words of one rule in failures.toml
        let all = self.all.iter().all(|word| lower.contains(word.as_str()));
        // complexity: bounded by the few words of one rule in failures.toml
        let any = self.any.iter().any(|word| lower.contains(word.as_str()));
        all && (self.any.is_empty() || any)
    }
}

#[derive(Deserialize)]
pub(super) struct Markers {
    window_after: Vec<String>,
    missing_model: Vec<Words>,
    pub(super) plan_cap: String,
    pub(super) plan_settings: String,
    named: Vec<Named>,
    malformed_tool_call: Vec<Words>,
    context_overflow: Vec<Words>,
    status: Vec<Status>,
    stop: Stops,
}

fn any(rules: &[Words], lower: &str) -> bool {
    rules
        .iter()
        // complexity: bounded by the word rules in failures.toml
        .any(|rule| rule.matches(lower))
}

static MARKERS: LazyLock<Markers> =
    LazyLock::new(|| summonr_core::embedded!("provider failure", "provider/client/failures.toml"));

pub(super) fn markers() -> &'static Markers {
    &MARKERS
}

impl Markers {
    pub(super) fn missing_model(&self, lower: &str) -> bool {
        any(&self.missing_model, lower)
    }

    pub(super) fn named(&self, lower: &str) -> Option<(&str, ProviderFailureKind)> {
        self.named
            .iter()
            // complexity: bounded by the named provider failure markers in failures.toml
            .find(|named| lower.contains(named.marker.as_str()))
            .map(|named| (named.marker.as_str(), named.kind.kind()))
    }

    pub(super) fn kind(&self, code: u16, lower: &str) -> ProviderFailureKind {
        match code {
            401 | 403 => ProviderFailureKind::Authentication,
            429 => ProviderFailureKind::RateLimited,
            400 | 500..=599 if any(&self.malformed_tool_call, lower) => {
                ProviderFailureKind::MalformedToolCall
            }
            500..=599 => ProviderFailureKind::Server,
            400 if any(&self.context_overflow, lower) => ProviderFailureKind::ContextOverflow,
            _ => ProviderFailureKind::Http,
        }
    }
}

pub(crate) fn status_code(kind: &str) -> u16 {
    let lower = kind.to_ascii_lowercase();
    markers()
        .status
        .iter()
        // complexity: bounded by the status markers in failures.toml
        .find(|status| lower.contains(status.marker.as_str()))
        .map_or(400, |status| status.code)
}

pub(crate) fn input_cap(text: &str) -> Option<u32> {
    let lower = text.to_ascii_lowercase();
    for mark in &markers().window_after {
        if let Some(index) = lower.find(mark.as_str())
            && let Some(cap) = leading_number(&text[index + mark.len()..])
        {
            return Some(cap);
        }
    }
    text.rsplit_once("> ")
        .and_then(|(_, rest)| leading_number(rest))
}

fn leading_number(text: &str) -> Option<u32> {
    let digits: String = text
        .chars()
        .skip_while(|ch| ch.is_ascii_whitespace())
        .take_while(|ch| ch.is_ascii_digit())
        .collect();
    digits.parse().ok().filter(|value| *value > 0)
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Ending {
    Stop,
    ToolCalls,
    Length,
    Other,
    Blocked,
    Server,
    Http,
    MalformedToolCall,
}

#[derive(Deserialize)]
struct Stops {
    anthropic: HashMap<String, Ending>,
    completions: HashMap<String, Ending>,
    gemini: HashMap<String, Ending>,
    responses: HashMap<String, Ending>,
}

pub(crate) fn ending(wire: Wire, raw: &str, detail: &str) -> Result<Option<StopReason>> {
    let stops = &markers().stop;
    let table = match wire {
        Wire::Anthropic | Wire::AgentSdk => &stops.anthropic,
        Wire::Gemini => &stops.gemini,
        Wire::Responses => &stops.responses,
        Wire::Completions | Wire::Decisions => &stops.completions,
    };
    let Some(ending) = table.get(raw).or_else(|| table.get("*")) else {
        return Ok(None);
    };
    let failed = |kind| {
        Err(Error::provider(
            kind,
            format!("native response ended with {raw}"),
        ))
    };
    match ending {
        Ending::Stop => Ok(Some(StopReason::Stop)),
        Ending::ToolCalls => Ok(Some(StopReason::ToolCalls)),
        Ending::Length => Ok(Some(StopReason::Length)),
        Ending::Other => Ok(Some(StopReason::Other)),
        Ending::Blocked if detail.is_empty() => Err(crate::provider::wire::blocked(raw)),
        Ending::Blocked => Err(crate::provider::wire::blocked(&format!("{raw}: {detail}"))),
        Ending::Server => failed(ProviderFailureKind::Server),
        Ending::Http => failed(ProviderFailureKind::Http),
        Ending::MalformedToolCall => failed(ProviderFailureKind::MalformedToolCall),
    }
}
