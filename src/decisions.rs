use std::collections::BTreeMap;

pub mod digest;
pub mod eval;
pub mod patch;
pub mod sets;
pub mod sweep;

use serde::{Deserialize, Serialize};

use crate::endpoint::{Endpoint, Wire};
use crate::error::{Error, ProviderFailureKind, Result};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    Choice {
        instructions: String,
        criteria: BTreeMap<String, String>,
    },
    Score {
        instructions: String,
        criteria: Vec<String>,
    },
    Noul {
        instructions: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Answer {
    Choice {
        choice: String,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
    Score {
        score: f64,
        legend: BTreeMap<String, String>,
        probabilities: BTreeMap<String, f64>,
        confidence: f64,
    },
    Noul {
        noul: f64,
    },
}

impl Answer {
    pub fn confidence(&self) -> Option<f64> {
        match self {
            Self::Choice { confidence, .. } | Self::Score { confidence, .. } => Some(*confidence),
            Self::Noul { .. } => None,
        }
    }

    pub fn acts_above(&self, floor: f64) -> bool {
        self.confidence().is_some_and(|value| value >= floor)
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub cost: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Decision {
    pub model: String,
    pub answers: BTreeMap<String, Answer>,
    #[serde(default)]
    pub usage: Usage,
}

#[derive(Serialize)]
struct Ask<'a> {
    model: &'a str,
    state: &'a str,
    questions: &'a BTreeMap<String, Question>,
}

pub async fn ask(
    endpoint: &Endpoint,
    state: &str,
    questions: &BTreeMap<String, Question>,
) -> Result<Decision> {
    let (url, key) = target(endpoint, questions)?;
    let client = reqwest::Client::builder()
        .build()
        .map_err(|error| Error::provider(ProviderFailureKind::Transport, error.to_string()))?;
    let response = client
        .post(&url)
        .bearer_auth(key)
        .json(&Ask {
            model: &endpoint.model,
            state,
            questions,
        })
        .send()
        .await
        .map_err(|error| Error::provider(ProviderFailureKind::Transport, error.to_string()))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| Error::provider(ProviderFailureKind::Transport, error.to_string()))?;
    if !status.is_success() {
        return Err(Error::Provider(crate::error::ProviderFailure {
            kind: classify(status.as_u16()),
            status: Some(status.as_u16()),
            message: message(&body),
            partial_output: false,
            retry_after: None,
        }));
    }
    serde_json::from_str(&body).map_err(|error| {
        Error::provider(
            ProviderFailureKind::Malformed,
            format!("{error}: {}", body.chars().take(200).collect::<String>()),
        )
    })
}

fn target<'a>(
    endpoint: &'a Endpoint,
    questions: &BTreeMap<String, Question>,
) -> Result<(String, &'a str)> {
    if endpoint.wire != Wire::Decisions {
        return Err(Error::Config(format!(
            "`{}` is not a decisions endpoint — see `summonr endpoints`",
            endpoint.name
        )));
    }
    if questions.is_empty() {
        return Err(Error::Config("ask at least one question".to_string()));
    }
    let base = endpoint
        .base_url
        .as_deref()
        .ok_or_else(|| Error::Config(format!("`{}` has no base URL", endpoint.name)))?;
    let key = endpoint.api_key.as_deref().ok_or_else(|| {
        Error::Config(format!(
            "no key for `{}` — set its environment variable, or `summon login {}`",
            endpoint.name, endpoint.name
        ))
    })?;
    Ok((base.to_string(), key))
}

fn classify(status: u16) -> ProviderFailureKind {
    match status {
        401 | 403 => ProviderFailureKind::Authentication,
        429 => ProviderFailureKind::RateLimited,
        500..=599 => ProviderFailureKind::Server,
        _ => ProviderFailureKind::Http,
    }
}

fn message(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(|error| error.get("message"))
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.chars().take(200).collect())
}

pub const CONTEXT_CHARS: usize = 84_000;

const WIDTH: usize = 20;

pub fn render(decision: &Decision) -> String {
    let mut out = String::new();
    for (name, answer) in &decision.answers {
        match answer {
            Answer::Choice {
                choice,
                probabilities,
                confidence,
            } => {
                out.push_str(&headline(name, choice, Some(*confidence)));
                out.push_str(&bars(probabilities, |label| label.to_string()));
            }
            Answer::Score {
                score,
                legend,
                probabilities,
                confidence,
            } => {
                let level = legend
                    .get(&format!("{}", score.round() as i64))
                    .map_or_else(
                        || format!("{score:.2}"),
                        |text| format!("{score:.2} {text}"),
                    );
                out.push_str(&headline(name, &level, Some(*confidence)));
                out.push_str(&bars(probabilities, |label| {
                    legend
                        .get(label)
                        .map_or_else(|| label.to_string(), |text| format!("{label} {text}"))
                }));
            }
            Answer::Noul { noul } => {
                let verdict = if *noul >= 0.5 { "yes" } else { "no" };
                out.push_str(&format!("{name:<16} {verdict:<28} p {noul:.2}\n"));
            }
        }
    }
    out
}

fn headline(name: &str, value: &str, confidence: Option<f64>) -> String {
    let value = clip(value, 28);
    match confidence {
        Some(confidence) => format!("{name:<16} {value:<28} conf {confidence:.2}\n"),
        None => format!("{name:<16} {value}\n"),
    }
}

fn clip(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    text.chars()
        .take(width.saturating_sub(1))
        .collect::<String>()
        + "…"
}

fn bars(probabilities: &BTreeMap<String, f64>, label: impl Fn(&str) -> String) -> String {
    let mut rows: Vec<_> = probabilities.iter().collect();
    rows.sort_by(|left, right| right.1.total_cmp(left.1));
    let mut out = String::new();
    for (key, share) in rows {
        let filled = (share * WIDTH as f64).round().clamp(0.0, WIDTH as f64) as usize;
        let bar: String = "█".repeat(filled) + &"░".repeat(WIDTH - filled);
        out.push_str(&format!(
            "  {:<18} {bar} {share:.2}\n",
            clip(&label(key), 18)
        ));
    }
    out
}

#[cfg(test)]
#[path = "decisions_tests.rs"]
mod tests;
