use std::collections::BTreeMap;

use super::{Answer, Decision, Question};
use crate::endpoint::Endpoint;
use crate::error::Result;

pub struct Change {
    pub path: String,
    pub text: String,
}

pub fn split(patch: &str) -> Vec<Change> {
    let mut changes: Vec<Change> = Vec::new();
    for line in patch.lines() {
        if let Some(header) = line.strip_prefix("diff --git ") {
            changes.push(Change {
                path: named(header),
                text: format!("{line}\n"),
            });
            continue;
        }
        if let Some(current) = changes.last_mut() {
            current.text.push_str(line);
            current.text.push('\n');
        }
    }
    changes
}

fn named(header: &str) -> String {
    header.split_whitespace().next_back().map_or_else(
        || header.to_string(),
        |path| path.strip_prefix("b/").unwrap_or(path).to_string(),
    )
}

pub async fn review(
    endpoint: &Endpoint,
    changes: &[Change],
    questions: &BTreeMap<String, Question>,
    budget: usize,
) -> Result<Vec<Decision>> {
    let asked = futures::future::join_all(
        changes
            .iter()
            .map(|change| super::sweep::sweep(endpoint, &change.text, questions, budget))
            .collect::<Vec<_>>(),
    )
    .await;
    let mut decided = Vec::new();
    for outcome in asked {
        decided.push(outcome?.0);
    }
    Ok(decided)
}

fn severity(decision: &Decision) -> f64 {
    decision
        .answers
        .get("risk")
        .and_then(|answer| match answer {
            Answer::Score { score, .. } => Some(*score),
            _ => None,
        })
        .unwrap_or_default()
}

fn flags(decision: &Decision, floor: f64) -> Vec<String> {
    let mut raised: Vec<String> = decision
        .answers
        .iter()
        .filter_map(|(name, answer)| match answer {
            Answer::Noul { noul } if *noul >= floor => Some(format!("{name} {noul:.2}")),
            _ => None,
        })
        .collect();
    raised.sort();
    raised
}

pub fn table(changes: &[Change], decided: &[Decision], floor: f64) -> String {
    let mut rows: Vec<(&Change, &Decision)> = changes.iter().zip(decided).collect();
    rows.sort_by(|left, right| severity(right.1).total_cmp(&severity(left.1)));
    let mut out = String::new();
    for (change, decision) in rows {
        let kind = decision
            .answers
            .get("kind")
            .and_then(|answer| match answer {
                Answer::Choice { choice, .. } => Some(choice.as_str()),
                _ => None,
            })
            .unwrap_or("-");
        out.push_str(&format!(
            "{:<44} {:<10} risk {:.2}\n",
            clip(&change.path, 44),
            kind,
            severity(decision)
        ));
        let raised = flags(decision, floor);
        if !raised.is_empty() {
            out.push_str(&format!("  {}\n", raised.join("   ")));
        }
    }
    out
}

fn clip(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        return text.to_string();
    }
    let tail: String = text.chars().skip(count - width.saturating_sub(1)).collect();
    format!("…{tail}")
}

#[cfg(test)]
#[path = "patch_tests.rs"]
mod tests;
