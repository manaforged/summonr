use std::collections::BTreeMap;

pub mod digest;
pub mod eval;
pub mod patch;
pub mod sets;
pub mod sweep;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::endpoint::{Endpoint, Wire};
use crate::error::{Error, ProviderFailureKind, Result};

pub(crate) const AT_ONCE: usize = 8;

pub(crate) async fn in_flight<T, F>(work: Vec<F>) -> Result<Vec<T>>
where
    F: std::future::Future<Output = Result<T>>,
{
    use futures::stream::StreamExt;
    let mut done = Vec::with_capacity(work.len());
    let mut stream = futures::stream::iter(work).buffered(AT_ONCE);
    while let Some(outcome) = stream.next().await {
        done.push(outcome?);
    }
    Ok(done)
}

pub(crate) const MOST_OPTIONS: usize = 255;
pub(crate) const FEWEST_LEVELS: usize = 2;
pub(crate) const MOST_LEVELS: usize = 10;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Boundary {
    #[serde(rename = "true")]
    pub yes: Value,
    #[serde(rename = "false")]
    pub no: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Question {
    Choice {
        instructions: Value,
        criteria: BTreeMap<String, Value>,
    },
    Score {
        instructions: Value,
        criteria: Vec<Value>,
    },
    Noul {
        instructions: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        criteria: Option<Boundary>,
    },
}

impl Question {
    pub fn noul(instructions: impl Into<Value>) -> Self {
        Self::Noul {
            instructions: instructions.into(),
            criteria: None,
        }
    }

    pub(crate) fn bounded(
        instructions: impl Into<Value>,
        yes: impl Into<Value>,
        no: impl Into<Value>,
    ) -> Self {
        Self::Noul {
            instructions: instructions.into(),
            criteria: Some(Boundary {
                yes: yes.into(),
                no: no.into(),
            }),
        }
    }

    pub(crate) fn choice<K, V>(
        instructions: impl Into<Value>,
        options: impl IntoIterator<Item = (K, V)>,
    ) -> Self
    where
        K: Into<String>,
        V: Into<Value>,
    {
        Self::Choice {
            instructions: instructions.into(),
            criteria: options
                .into_iter()
                .map(|(name, about)| (name.into(), about.into()))
                .collect(),
        }
    }

    pub fn score(
        instructions: impl Into<Value>,
        levels: impl IntoIterator<Item = impl Into<Value>>,
    ) -> Self {
        Self::Score {
            instructions: instructions.into(),
            criteria: levels.into_iter().map(Into::into).collect(),
        }
    }

    pub fn check(&self, name: &str) -> Result<()> {
        match self {
            Self::Choice { criteria, .. } => {
                if criteria.is_empty() || criteria.len() > MOST_OPTIONS {
                    return Err(Error::Config(format!(
                        "`{name}` offers {} options; a choice takes 1 to {MOST_OPTIONS}",
                        criteria.len()
                    )));
                }
            }
            Self::Score { criteria, .. } => {
                if criteria.len() < FEWEST_LEVELS || criteria.len() > MOST_LEVELS {
                    return Err(Error::Config(format!(
                        "`{name}` has {} levels; a score takes {FEWEST_LEVELS} to {MOST_LEVELS}",
                        criteria.len()
                    )));
                }
            }
            Self::Noul { .. } => {}
        }
        Ok(())
    }
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

pub async fn ask(
    endpoint: &Endpoint,
    state: &Value,
    questions: &BTreeMap<String, Question>,
) -> Result<Decision> {
    target(endpoint, questions)?;
    let client = crate::provider::Client::connect(endpoint)?;
    let decision = client.classify(&endpoint.model, state, questions).await?;
    for (name, question) in questions {
        if !matches!(
            (question, decision.answers.get(name)),
            (Question::Choice { .. }, Some(Answer::Choice { .. }))
                | (Question::Score { .. }, Some(Answer::Score { .. }))
                | (Question::Noul { .. }, Some(Answer::Noul { .. }))
        ) {
            return Err(Error::provider(
                ProviderFailureKind::Malformed,
                format!("missing or invalid answer for `{name}`"),
            ));
        }
    }
    Ok(decision)
}

fn target(endpoint: &Endpoint, questions: &BTreeMap<String, Question>) -> Result<()> {
    if endpoint.wire != Wire::Decisions {
        return Err(Error::Config(format!(
            "`{}` is not a classify endpoint — see `summonr endpoints`",
            endpoint.name
        )));
    }
    if questions.is_empty() {
        return Err(Error::Config("ask at least one question".to_string()));
    }
    for (name, question) in questions {
        question.check(name)?;
    }
    if endpoint
        .base_url
        .as_deref()
        .is_none_or(|base| base.trim().is_empty())
    {
        return Err(Error::Config(format!(
            "`{}` has no base URL",
            endpoint.name
        )));
    }
    if endpoint
        .api_key
        .as_deref()
        .is_none_or(|key| key.trim().is_empty())
        && endpoint.oauth.is_none()
    {
        return Err(Error::Config(format!(
            "no key for `{}` — set its environment variable, or `summonr login {}`",
            endpoint.name, endpoint.name
        )));
    }
    Ok(())
}

pub(crate) const CONTEXT_CHARS: usize = 84_000;
pub(crate) const CHARS_PER_TOKEN: usize = 3;

pub fn budget(endpoint: &Endpoint, questions: &BTreeMap<String, Question>) -> usize {
    let room = endpoint
        .context_window
        .map_or(CONTEXT_CHARS, |tokens| tokens as usize * CHARS_PER_TOKEN);
    let asked = serde_json::to_string(questions).map_or(0, |text| text.len());
    room.saturating_sub(asked)
}

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
    let value = crate::text::ellipsize(value, 28);
    match confidence {
        Some(confidence) => format!("{name:<16} {value:<28} conf {confidence:.2}\n"),
        None => format!("{name:<16} {value}\n"),
    }
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
            crate::text::ellipsize(&label(key), 18)
        ));
    }
    out
}

#[cfg(test)]
#[path = "classify_tests.rs"]
pub(crate) mod tests;

#[cfg(test)]
#[path = "classify/question_tests.rs"]
mod question_tests;
