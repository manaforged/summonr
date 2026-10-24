use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::verify::ScoreConfig;

mod ledger;
mod run;
mod spend;

pub(crate) use run::{Measured, board, measure, watch};
pub(crate) use spend::{Goal, Reading, Spend};

const LEDGER_ROWS: usize = 8;
const CHANGES_SHOWN: usize = 6;
pub(crate) const FAILED_TURNS: usize = 3;
pub(crate) const RESTART_TURNS: usize = 3;
pub(crate) const PARK_TURNS: usize = 6;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct Entry {
    pub(crate) time: String,
    #[serde(default)]
    pub(crate) session: Option<String>,
    #[serde(default)]
    pub(crate) head: Option<String>,
    #[serde(default)]
    pub(crate) changes: Vec<String>,
    pub(crate) scores: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) spend: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) spend_start: Option<f64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) halted: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub(crate) struct Row {
    pub(crate) name: String,
    pub(crate) current: Option<f64>,
    pub(crate) best: Option<f64>,
    pub(crate) target: Option<f64>,
    pub(crate) gap: Option<f64>,
    pub(crate) met: Option<bool>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub(crate) struct Board {
    pub(crate) ledger: PathBuf,
    pub(crate) rows: Vec<Row>,
    pub(crate) met: bool,
    pub(crate) unmet: Vec<String>,
    pub(crate) turns_since_best: usize,
    pub(crate) stalled: bool,
    pub(crate) stall: Option<String>,
    pub(crate) entries: Vec<Entry>,
    pub(crate) spend: Option<Spend>,
    pub(crate) failed: usize,
    pub(crate) quiet: usize,
}

impl Entry {
    pub(crate) fn new(
        scores: BTreeMap<String, f64>,
        error: Option<String>,
        session: Option<String>,
        workspace: &Path,
    ) -> Self {
        let (head, changes) = git_state(workspace);
        Self {
            time: jiff::Timestamp::now().to_string(),
            session,
            head,
            changes,
            scores,
            error,
            spend: None,
            spend_start: None,
            halted: false,
        }
    }
}

pub(crate) fn tree(workspace: &Path, session: Option<&str>) -> Option<String> {
    let points = crate::checkpoint::Checkpoints::open(workspace, session?)?;
    crate::tools::blocking(|| points.tree().ok())
}

fn git_state(workspace: &Path) -> (Option<String>, Vec<String>) {
    if !crate::diff::inside_git(workspace) {
        return (None, Vec::new());
    }
    crate::tools::blocking(|| {
        let head = crate::diff::git_env(workspace, &[], &["rev-parse", "HEAD"])
            .ok()
            .map(|head| head.trim().to_string())
            .filter(|head| !head.is_empty());
        let changes = crate::diff::stats(workspace)
            .map(|diff| diff.files.into_iter().map(|stat| stat.path).collect())
            .unwrap_or_default();
        (head, changes)
    })
}

fn raise(best: &mut BTreeMap<String, f64>, name: &str, value: f64) -> bool {
    if best.get(name).is_some_and(|old| *old >= value) {
        return false;
    }
    best.insert(name.to_string(), value);
    true
}

impl Board {
    pub(crate) fn new(config: &ScoreConfig, ledger: PathBuf, entries: &[Entry]) -> Self {
        let mut best = BTreeMap::new();
        let mut improved_at = BTreeMap::new();
        let scored: Vec<&Entry> = entries
            .iter()
            .filter(|entry| !entry.scores.is_empty())
            .collect();
        for (index, entry) in scored.iter().enumerate() {
            for (name, value) in &entry.scores {
                if raise(&mut best, name, *value) {
                    improved_at.insert(name.clone(), index);
                }
            }
        }
        let current = scored
            .last()
            .map(|entry| entry.scores.clone())
            .unwrap_or_default();
        let mut names: BTreeSet<String> = current.keys().cloned().collect();
        names.extend(config.targets.keys().cloned());
        names.extend(best.keys().cloned());
        let rows: Vec<Row> = names
            .into_iter()
            .map(|name| row(name, &current, &best, &config.targets))
            .collect();
        let unmet: Vec<String> = rows
            .iter()
            .filter(|row| row.met == Some(false))
            .map(|row| row.name.clone())
            .collect();
        let last_gain = unmet
            .iter()
            .filter_map(|name| improved_at.get(name).copied())
            .max();
        let turns_since_best = match last_gain {
            Some(index) => scored.len() - 1 - index,
            None => scored.len(),
        };
        let stalled =
            !unmet.is_empty() && turns_since_best >= crate::limits::limits().score_stall_turns;
        let stall = stalled.then(|| stall_line(&scored, turns_since_best));
        let shown = entries.len().saturating_sub(LEDGER_ROWS);
        Self {
            ledger,
            met: unmet.is_empty(),
            rows,
            unmet,
            turns_since_best,
            stalled,
            stall,
            entries: entries[shown..].to_vec(),
            spend: None,
            failed: 0,
            quiet: 0,
        }
    }

    #[must_use]
    pub(crate) fn text(&self) -> String {
        let mut text = format!("Scoreboard (ledger `{}`)\n", self.ledger.display());
        text.push_str("name\tcurrent\tbest\ttarget\tgap\tmet\n");
        for row in &self.rows {
            text.push_str(&format!(
                "{}\t{}\t{}\t{}\t{}\t{}\n",
                row.name,
                number(row.current),
                number(row.best),
                number(row.target),
                number(row.gap),
                match row.met {
                    Some(true) => "yes",
                    Some(false) => "no",
                    None => "-",
                },
            ));
        }
        if !self.unmet.is_empty() {
            text.push_str(&format!(
                "Unmet targets: {}. Scored turns since the best of an unmet target improved: {}.\n",
                self.unmet.join(", "),
                self.turns_since_best
            ));
        }
        if let Some(stall) = &self.stall {
            text.push_str(stall);
            text.push('\n');
        }
        if self.failed > 0 {
            text.push_str(&format!(
                "The score command failed in the last {} measurements of this goal; the goal stops at {FAILED_TURNS}. Fix the measurement first.\n",
                self.failed
            ));
        }
        if !self.rows.is_empty() {
            text.push_str(&evidence(self.quiet));
            text.push('\n');
        }
        if let Some(spend) = &self.spend {
            text.push_str(&spend.line());
            text.push('\n');
        }
        if !self.entries.is_empty() {
            text.push_str("Recent ledger entries:\n");
            for entry in &self.entries {
                text.push_str(&format!("- {}\n", entry_line(entry)));
            }
        }
        text.trim_end().to_string()
    }
}

pub(crate) fn evidence(quiet: usize) -> String {
    let lead = format!("Goal turns since a new measurement: {quiet}.");
    if quiet >= RESTART_TURNS {
        return format!(
            "{lead} The current approach is not producing evidence: drop it, reread this board, pick the change most likely to move an unmet target, and measure it this turn. At {PARK_TURNS} the goal parks."
        );
    }
    format!(
        "{lead} Tests, commits, and checks you run yourself are not progress, and neither is a rerun with no code change; only a measured change after a code change is. At {PARK_TURNS} the goal parks."
    )
}

fn row(
    name: String,
    current: &BTreeMap<String, f64>,
    best: &BTreeMap<String, f64>,
    targets: &BTreeMap<String, f64>,
) -> Row {
    let value = current.get(&name).copied();
    let target = targets.get(&name).copied();
    Row {
        current: value,
        best: best.get(&name).copied(),
        target,
        gap: target.zip(value).map(|(target, value)| target - value),
        met: target.map(|target| value.is_some_and(|value| value >= target)),
        name,
    }
}

fn stall_line(entries: &[&Entry], turns: usize) -> String {
    let flat: Vec<String> = entries[entries.len() - turns..]
        .iter()
        .map(|entry| format!("{} at {}", entry.time, short_head(entry)))
        .collect();
    format!(
        "Stalled: no unmet target improved in the last {turns} scored turns ({}). Change the mechanism, not the parameters.",
        flat.join("; ")
    )
}

fn entry_line(entry: &Entry) -> String {
    let scores: Vec<String> = entry
        .scores
        .iter()
        .map(|(name, value)| format!("{name}={}", number(Some(*value))))
        .collect();
    let mut line = format!("{} {} {}", entry.time, short_head(entry), scores.join(" "));
    if let Some(spend) = entry.spend {
        line.push_str(&format!(" spend=${}", number(Some(spend))));
    }
    if let Some(error) = &entry.error {
        line.push_str(&format!("score failed: {error}"));
    }
    if !entry.changes.is_empty() {
        let mut changes = entry.changes[..entry.changes.len().min(CHANGES_SHOWN)].join(", ");
        if entry.changes.len() > CHANGES_SHOWN {
            changes.push_str(&format!(", … {} more", entry.changes.len() - CHANGES_SHOWN));
        }
        line.push_str(&format!(" (changed: {changes})"));
    }
    line
}

fn short_head(entry: &Entry) -> &str {
    entry
        .head
        .as_deref()
        .map_or("no commit", |head| head.get(..12).unwrap_or(head))
}

fn number(value: Option<f64>) -> String {
    value.map_or_else(
        || "-".to_string(),
        |value| {
            let text = format!("{value:.4}");
            text.trim_end_matches('0').trim_end_matches('.').to_string()
        },
    )
}

fn body(output: &str) -> &str {
    output.strip_prefix("exit ").map_or(output, |rest| {
        rest.split_once('\n').map_or("", |(_, body)| body)
    })
}

pub(crate) fn parse(output: &str) -> Option<BTreeMap<String, f64>> {
    let body = body(output);
    if let Ok(scores) = serde_json::from_str::<BTreeMap<String, f64>>(body.trim()) {
        return finite(scores);
    }
    let mut scores = BTreeMap::new();
    for line in body.lines() {
        let mut words = line.split_whitespace();
        if let (Some(name), Some(value), None) = (words.next(), words.next(), words.next())
            && let Ok(value) = value.parse::<f64>()
        {
            scores.insert(name.to_string(), value);
        }
    }
    finite(scores)
}

fn finite(mut scores: BTreeMap<String, f64>) -> Option<BTreeMap<String, f64>> {
    scores.retain(|_, value| value.is_finite());
    (!scores.is_empty()).then_some(scores)
}

#[cfg(test)]
#[path = "score_tests.rs"]
mod tests;
