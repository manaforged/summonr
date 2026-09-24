use serde::Deserialize;
use std::collections::BTreeMap;

use super::{Answer, Decision, Question};
use crate::endpoint::Endpoint;
use crate::error::{Error, Result};

#[derive(Clone, Debug, Deserialize)]
pub struct Example {
    pub state: serde_json::Value,
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
        if example.expect.is_empty() {
            return Err(Error::Config(format!(
                "line {}: `expect` needs at least one label",
                index + 1
            )));
        }
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
    let verdict = match answer {
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
            if !got.is_finite() || !wanted.is_finite() {
                return None;
            }
            Some(Verdict {
                right: (got - wanted).abs() <= 0.5,
                wanted: format!("{wanted:.2}"),
                got: format!("{got:.2}"),
                certainty: confidence.to_owned(),
            })
        }
    }?;
    (0.0..=1.0).contains(&verdict.certainty).then_some(verdict)
}

pub fn score(
    examples: &[Example],
    decided: &[Decision],
    floor: f64,
) -> Result<(BTreeMap<String, Tally>, Vec<Miss>)> {
    if !(0.0..=1.0).contains(&floor) {
        return Err(Error::Config(
            "evaluation confidence must be from 0 to 1".to_string(),
        ));
    }
    if examples.is_empty() || examples.len() != decided.len() {
        return Err(Error::Config(format!(
            "evaluation needs one decision per example: {} examples, {} decisions",
            examples.len(),
            decided.len()
        )));
    }
    let mut tallies: BTreeMap<String, Tally> = BTreeMap::new();
    let mut misses = Vec::new();
    for (index, (example, decision)) in examples.iter().zip(decided).enumerate() {
        let label = example
            .name
            .clone()
            .unwrap_or_else(|| format!("example {}", index + 1));
        if example.expect.is_empty() {
            return Err(Error::Config(format!(
                "{label}: `expect` needs at least one label"
            )));
        }
        for (question, wanted) in &example.expect {
            let answer = decision.answers.get(question).ok_or_else(|| {
                Error::Config(format!("{label}: missing answer for `{question}`"))
            })?;
            let verdict = judge(answer, wanted).ok_or_else(|| {
                Error::Config(format!("{label}: invalid answer or label for `{question}`"))
            })?;
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
    Ok((tallies, misses))
}

pub async fn run(
    endpoint: &Endpoint,
    examples: &[Example],
    questions: &BTreeMap<String, Question>,
    budget: usize,
) -> Result<Vec<Decision>> {
    if examples.is_empty() || questions.is_empty() {
        return Err(Error::Config(
            "evaluation needs examples and questions".to_string(),
        ));
    }
    for (name, question) in questions {
        question.check(name)?;
    }
    for (index, example) in examples.iter().enumerate() {
        if example.expect.is_empty() {
            return Err(Error::Config(format!(
                "example {}: `expect` needs at least one label",
                index + 1
            )));
        }
        for (name, wanted) in &example.expect {
            let valid = match questions.get(name) {
                Some(Question::Choice { criteria, .. }) => wanted
                    .as_str()
                    .is_some_and(|label| criteria.contains_key(label)),
                Some(Question::Score { criteria, .. }) => wanted
                    .as_f64()
                    .is_some_and(|score| (0.0..=(criteria.len() - 1) as f64).contains(&score)),
                Some(Question::Noul { .. }) => wanted.is_boolean(),
                None => false,
            };
            if !valid {
                return Err(Error::Config(format!(
                    "example {}: invalid label for question `{name}`",
                    index + 1
                )));
            }
        }
    }
    let swept = super::in_flight(
        examples
            .iter()
            .map(|example| super::sweep::sweep(endpoint, &example.state, questions, budget))
            .collect(),
    )
    .await?;
    Ok(swept.into_iter().map(|(decision, _)| decision).collect())
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
