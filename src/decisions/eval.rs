use serde::Deserialize;
use std::collections::BTreeMap;

use super::{Answer, Decision, Question};
use crate::endpoint::Endpoint;
use crate::error::{Error, Result};

#[derive(Clone, Debug, Deserialize)]
pub struct Example {
    pub state: String,
    pub expect: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub name: Option<String>,
}

pub fn load(text: &str) -> Result<Vec<Example>> {
    let mut examples = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let example: Example = serde_json::from_str(line)
            .map_err(|error| Error::Config(format!("line {}: {error}", index + 1)))?;
        examples.push(example);
    }
    if examples.is_empty() {
        return Err(Error::Config(
            "no examples: each line is one JSON object with `state` and `expect`".to_string(),
        ));
    }
    Ok(examples)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub asked: usize,
    pub right: usize,
    pub acted: usize,
    pub acted_right: usize,
}

impl Tally {
    pub fn accuracy(&self) -> f64 {
        if self.asked == 0 {
            return 0.0;
        }
        self.right as f64 / self.asked as f64
    }

    pub fn gated_accuracy(&self) -> f64 {
        if self.acted == 0 {
            return 0.0;
        }
        self.acted_right as f64 / self.acted as f64
    }
}

pub struct Miss {
    pub example: String,
    pub question: String,
    pub wanted: String,
    pub got: String,
    pub certainty: f64,
}

pub struct Verdict {
    pub right: bool,
    pub wanted: String,
    pub got: String,
    pub certainty: f64,
}

pub fn judge(answer: &Answer, wanted: &serde_json::Value) -> Option<Verdict> {
    match answer {
        Answer::Choice {
            choice, confidence, ..
        } => {
            let wanted = wanted.as_str()?;
            Some(Verdict {
                right: choice == wanted,
                wanted: wanted.to_string(),
                got: choice.clone(),
                certainty: confidence.to_owned(),
            })
        }
        Answer::Noul { noul } => {
            let wanted = wanted.as_bool()?;
            let probability = noul.to_owned();
            let got = probability >= 0.5;
            let certainty = if got { probability } else { 1.0 - probability };
            Some(Verdict {
                right: got == wanted,
                wanted: wanted.to_string(),
                got: got.to_string(),
                certainty,
            })
        }
        Answer::Score {
            score, confidence, ..
        } => {
            let wanted = wanted.as_f64()?;
            let got = score.to_owned();
            Some(Verdict {
                right: (got - wanted).abs() <= 0.5,
                wanted: format!("{wanted:.2}"),
                got: format!("{got:.2}"),
                certainty: confidence.to_owned(),
            })
        }
    }
}

pub fn score(
    examples: &[Example],
    decided: &[Decision],
    floor: f64,
) -> (BTreeMap<String, Tally>, Vec<Miss>) {
    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    let mut misses = Vec::new();
    for (index, (example, decision)) in examples.iter().zip(decided).enumerate() {
        let label = example
            .name
            .clone()
            .unwrap_or_else(|| format!("example {}", index + 1));
        for (question, wanted) in &example.expect {
            let Some(answer) = decision.answers.get(question) else {
                continue;
            };
            let Some(verdict) = judge(answer, wanted) else {
                continue;
            };
            let tally = tallies.entry(question.clone()).or_default();
            tally.asked += 1;
            if verdict.right {
                tally.right += 1;
            }
            if verdict.certainty >= floor {
                tally.acted += 1;
                if verdict.right {
                    tally.acted_right += 1;
                }
            }
            if !verdict.right {
                misses.push(Miss {
                    example: label.clone(),
                    question: question.clone(),
                    wanted: verdict.wanted,
                    got: verdict.got,
                    certainty: verdict.certainty,
                });
            }
        }
    }
    (tallies, misses)
}

pub async fn run(
    endpoint: &Endpoint,
    examples: &[Example],
    questions: &BTreeMap<String, Question>,
    budget: usize,
) -> Result<Vec<Decision>> {
    let asked = futures::future::join_all(
        examples
            .iter()
            .map(|example| super::sweep::sweep(endpoint, &example.state, questions, budget))
            .collect::<Vec<_>>(),
    )
    .await;
    let mut decided = Vec::new();
    for outcome in asked {
        decided.push(outcome?.0);
    }
    Ok(decided)
}

pub fn report(tallies: &BTreeMap<String, Tally>, misses: &[Miss], floor: f64) -> String {
    let mut out = format!(
        "{:<18} {:>8} {:>9} {:>9}\n",
        "question", "answered", "correct", "gated"
    );
    for (question, tally) in tallies {
        out.push_str(&format!(
            "{:<18} {:>8} {:>8.0}% {:>8.0}%  ({} of {} at or above {floor:.2})\n",
            question,
            tally.asked,
            tally.accuracy() * 100.0,
            tally.gated_accuracy() * 100.0,
            tally.acted_right,
            tally.acted,
        ));
    }
    if !misses.is_empty() {
        out.push_str("\nmisses\n");
        for miss in misses {
            out.push_str(&format!(
                "  {:<14} {:<14} wanted {:<12} got {:<12} certainty {:.2}\n",
                miss.example, miss.question, miss.wanted, miss.got, miss.certainty
            ));
        }
    }
    out
}

#[cfg(test)]
#[path = "eval_tests.rs"]
mod tests;
