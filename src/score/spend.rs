use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::{Entry, body, number, parse};
use crate::tools::ToolRegistry;
use crate::verify::ScoreConfig;

const SPEND: &str = "spend";

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Reading {
    Scores(BTreeMap<String, f64>),
    Output(String),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Goal {
    pub(crate) since: Option<jiff::Timestamp>,
    pub(crate) start: Option<f64>,
    pub(crate) last_spend: Option<Instant>,
    pub(crate) tree: Option<String>,
    pub(crate) last: Option<Reading>,
    pub(crate) noise: BTreeMap<String, f64>,
    pub(crate) quiet: usize,
    pub(crate) todos: Option<Vec<crate::tools::Todo>>,
    pub(crate) check: Option<String>,
    pub(crate) reopened: BTreeSet<usize>,
    pub(crate) task_tree: Option<String>,
    pub(crate) notes: Vec<String>,
}

impl Goal {
    #[must_use]
    pub(crate) fn started() -> Self {
        Self {
            since: Some(jiff::Timestamp::now()),
            ..Self::default()
        }
    }

    #[must_use]
    pub(crate) fn owns(&self, entry: &Entry, session: Option<&str>) -> bool {
        entry.session.as_deref() == session
            && self.since.is_none_or(|since| {
                entry
                    .time
                    .parse::<jiff::Timestamp>()
                    .is_ok_and(|time| time >= since)
            })
    }

    pub(crate) fn observe(&mut self, tree: Option<String>, reading: Reading) {
        let same = tree.is_some() && tree == self.tree;
        let fresh = match (&self.last, &reading) {
            (None, _) => true,
            (Some(Reading::Scores(last)), Reading::Scores(now)) if same => {
                for (name, value) in now {
                    if let Some(old) = last.get(name) {
                        let spread = self.noise.entry(name.clone()).or_insert(0.0);
                        *spread = spread.max((value - old).abs());
                    }
                }
                false
            }
            (Some(Reading::Scores(last)), Reading::Scores(now)) => {
                last.keys().any(|name| !now.contains_key(name))
                    || now.iter().any(|(name, value)| {
                        last.get(name).is_none_or(|old| {
                            (value - old).abs() > self.noise.get(name).copied().unwrap_or(0.0)
                        })
                    })
            }
            (Some(last), now) => !same && last != now,
        };
        self.quiet = if fresh { 0 } else { self.quiet + 1 };
        self.tree = tree;
        self.last = Some(reading);
    }

    pub(crate) fn spend_due(&mut self) -> bool {
        let watch = Duration::from_secs(crate::limits::limits().score_watch_secs);
        if self.last_spend.is_some_and(|last| last.elapsed() < watch) {
            return false;
        }
        self.last_spend = Some(Instant::now());
        true
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub(crate) struct Spend {
    pub(crate) current: Option<f64>,
    pub(crate) start: Option<f64>,
    pub(crate) budget: Option<f64>,
    pub(crate) unavailable: Option<String>,
    pub(crate) halted: bool,
    pub(crate) halt: Option<String>,
}

impl Spend {
    #[must_use]
    pub(crate) fn added(&self) -> Option<f64> {
        self.current
            .zip(self.start)
            .map(|(current, start)| current - start)
    }

    #[must_use]
    pub(crate) fn breached(&self) -> bool {
        self.added()
            .zip(self.budget)
            .is_some_and(|(added, budget)| added >= budget)
    }

    #[must_use]
    pub(crate) fn line(&self) -> String {
        if let Some(reason) = &self.unavailable {
            return format!("spend unavailable: {reason}");
        }
        let added = number(self.added());
        match self.budget {
            Some(budget) => format!(
                "spend: ${added} of ${} budget since the goal started",
                number(Some(budget))
            ),
            None => format!("spend: ${added} since the goal started"),
        }
    }

    #[must_use]
    pub(crate) fn reason(&self) -> String {
        format!(
            "budget reached: ${} of ${}",
            number(self.added()),
            number(self.budget)
        )
    }

    #[must_use]
    pub(crate) fn note(&self) -> String {
        let halt = match &self.halt {
            Some(line) => line.as_str(),
            None if self.halted => "halt already ran in this goal",
            None => "no `halt` command is configured",
        };
        format!("{}; {halt}", self.reason())
    }

    pub(crate) fn record(&self, entry: &mut Entry) {
        entry.spend = self.current;
        entry.spend_start = self.start;
        entry.halted = self.halted;
    }
}

fn value(output: &str) -> Result<f64, String> {
    if let Ok(value) = body(output).trim().parse::<f64>()
        && value.is_finite()
    {
        return Ok(value);
    }
    parse(output)
        .and_then(|scores| scores.get(SPEND).copied())
        .ok_or_else(|| "the spend command printed no number and no `spend` line".to_string())
}

pub(super) async fn read(
    tools: &ToolRegistry,
    config: &ScoreConfig,
    entries: &[Entry],
    session: Option<&str>,
    goal: &mut Goal,
) -> Option<Spend> {
    let command = config.spend.as_ref()?;
    let result = tools.run_check(command).await;
    let outcome = crate::verify::outcome(command, &result);
    let reading = if outcome.passed {
        value(&outcome.output)
    } else {
        Err(outcome
            .output
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("the spend command failed")
            .to_string())
    };
    let mut mine = entries.iter().filter(|entry| goal.owns(entry, session));
    let start = mine
        .clone()
        .find_map(|entry| entry.spend_start.or(entry.spend));
    let halted = mine.any(|entry| entry.halted);
    let (current, unavailable) = match reading {
        Ok(current) => (Some(current), None),
        Err(reason) => (None, Some(reason)),
    };
    goal.start = start.or(goal.start).or(current);
    Some(Spend {
        current,
        start: goal.start,
        budget: config.budget,
        unavailable,
        halted,
        halt: None,
    })
}

pub(super) async fn halt(tools: &ToolRegistry, config: &ScoreConfig, spend: &mut Spend) {
    if spend.halted || !spend.breached() {
        return;
    }
    let Some(command) = config.halt.as_ref() else {
        return;
    };
    let result = tools.run_check(command).await;
    let outcome = crate::verify::outcome(command, &result);
    spend.halt = Some(if outcome.passed {
        format!("halt `{command}` exited 0")
    } else {
        format!(
            "halt `{command}` failed: {}",
            outcome.output.lines().next().unwrap_or("no output")
        )
    });
    spend.halted = true;
}
