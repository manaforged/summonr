use std::collections::BTreeMap;

use super::{Answer, Decision, Question, Usage, ask};
use crate::endpoint::Endpoint;
use crate::error::{Error, Result};

pub fn windows(state: &str, budget: usize) -> Vec<String> {
    if budget == 0 || state.chars().count() <= budget {
        return vec![state.to_string()];
    }
    let mut cut = Vec::new();
    let mut current = String::new();
    for line in state.split_inclusive('\n') {
        if current.chars().count() + line.chars().count() > budget && !current.is_empty() {
            cut.push(std::mem::take(&mut current));
        }
        if line.chars().count() > budget {
            let mut rest: Vec<char> = line.chars().collect();
            while rest.len() > budget {
                let tail = rest.split_off(budget);
                cut.push(rest.into_iter().collect());
                rest = tail;
            }
            current = rest.into_iter().collect();
            continue;
        }
        current.push_str(line);
    }
    if !current.is_empty() {
        cut.push(current);
    }
    cut
}

pub async fn sweep(
    endpoint: &Endpoint,
    state: &str,
    questions: &BTreeMap<String, Question>,
    budget: usize,
) -> Result<(Decision, usize)> {
    let parts = windows(state, budget);
    let count = parts.len();
    let asked = futures::future::join_all(
        parts
            .iter()
            .map(|part| ask(endpoint, part, questions))
            .collect::<Vec<_>>(),
    )
    .await;
    let mut seen = Vec::new();
    for outcome in asked {
        seen.push(outcome?);
    }
    Ok((merge(&seen)?, count))
}

pub fn merge(seen: &[Decision]) -> Result<Decision> {
    let (first, rest) = seen
        .split_first()
        .ok_or_else(|| Error::Config("no windows to merge".to_string()))?;
    if rest.is_empty() {
        return Ok(first.clone());
    }
    let mut answers: BTreeMap<String, Answer> = BTreeMap::new();
    for name in seen.iter().flat_map(|one| one.answers.keys()) {
        if answers.contains_key(name) {
            continue;
        }
        let found: Vec<&Answer> = seen
            .iter()
            .filter_map(|one| one.answers.get(name))
            .collect();
        if let Some(answer) = strongest(&found) {
            answers.insert(name.clone(), answer);
        }
    }
    Ok(Decision {
        model: first.model.clone(),
        answers,
        usage: Usage {
            input_tokens: seen.iter().map(|one| one.usage.input_tokens).sum(),
            output_tokens: seen.iter().map(|one| one.usage.output_tokens).sum(),
            cost: seen
                .iter()
                .filter_map(|one| one.usage.cost)
                .reduce(|total, each| total + each),
        },
    })
}

fn strongest(found: &[&Answer]) -> Option<Answer> {
    match found.first()? {
        Answer::Noul { .. } => {
            let highest = found
                .iter()
                .filter_map(|answer| match answer {
                    Answer::Noul { noul } => Some(*noul),
                    _ => None,
                })
                .reduce(f64::max)?;
            Some(Answer::Noul { noul: highest })
        }
        Answer::Score { .. } => found
            .iter()
            .filter_map(|answer| match answer {
                Answer::Score { score, .. } => Some((*score, *answer)),
                _ => None,
            })
            .max_by(|left, right| left.0.total_cmp(&right.0))
            .map(|(_, answer)| answer.clone()),
        Answer::Choice { .. } => blend(found),
    }
}

fn blend(found: &[&Answer]) -> Option<Answer> {
    let mut weighted: BTreeMap<String, f64> = BTreeMap::new();
    let mut best = 0.0_f64;
    let mut weight = 0.0_f64;
    for answer in found {
        let Answer::Choice {
            probabilities,
            confidence,
            ..
        } = answer
        else {
            continue;
        };
        let share = confidence.max(0.01);
        weight += share;
        best = best.max(*confidence);
        for (option, probability) in probabilities {
            let carried = weighted.get(option).copied().unwrap_or_default();
            weighted.insert(option.clone(), carried + probability * share);
        }
    }
    if weight <= 0.0 {
        return None;
    }
    let weighted: BTreeMap<String, f64> = weighted
        .into_iter()
        .map(|(option, probability)| (option, probability / weight))
        .collect();
    let choice = weighted
        .iter()
        .max_by(|left, right| left.1.total_cmp(right.1))
        .map(|(option, _)| option.clone())?;
    Some(Answer::Choice {
        choice,
        probabilities: weighted,
        confidence: best,
    })
}

#[cfg(test)]
#[path = "sweep_tests.rs"]
mod tests;
