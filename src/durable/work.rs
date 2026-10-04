use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};
use std::hash::{Hash, Hasher};
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::store;
use super::tree::{Snapshot, changed};

mod progress;
mod stage;

pub(crate) use progress::WorkPhase;
pub use progress::{WorkKind, WorkProgress};
pub(crate) use progress::{checks, roman};

const LEDGER_WIDTH: usize = 72;
const LEDGER_MAX: usize = 40;
const TREES: &str = "tree.json";
const DECISION: &str = "Goal Work requires a completion decision";

#[derive(Clone, Debug, Error, PartialEq, Eq)]
#[error("{0}")]
pub(crate) struct WorkError(pub(crate) String);

impl From<crate::Error> for WorkError {
    fn from(error: crate::Error) -> Self {
        Self(error.to_string())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Policy {
    Single,
    Steps,
    Goal {},
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Stage {
    Plan,
    Execute,
    Verify,
    Review,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    Active(Stage),
    Completed,
    Abandoned(String),
    Failed(String),
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StepState {
    Pending,
    Active,
    Completed,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Observation {
    File(String),
    Stdout(String),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Check {
    pub(crate) command: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) observation: Option<Observation>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Step {
    pub(crate) title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) phase: Option<String>,
    pub(crate) checks: Vec<usize>,
    pub(crate) state: StepState,
    pub(crate) attempts: u16,
    pub(crate) rounds: u32,
}

impl Step {
    fn new(title: &str, phase: Option<&str>, checks: Vec<usize>) -> Self {
        Self {
            title: title.to_string(),
            phase: phase.map(str::to_string),
            checks,
            state: StepState::Pending,
            attempts: 0,
            rounds: 0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Candidate {
    pub(crate) paths: Vec<String>,
    #[serde(default, skip_serializing)]
    pub(crate) snapshot: Snapshot,
}

pub(crate) enum Decision {
    Automatic,
    GoalComplete,
    GoalContinue(String),
    GoalStop(String),
}

#[derive(Deserialize, Hash, Serialize)]
struct Trees<T> {
    base: Option<T>,
    candidate: Option<T>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Work {
    pub(crate) objective: String,
    pub(crate) policy: Policy,
    pub(crate) status: Status,
    pub(crate) plan: Option<String>,
    pub(crate) checks: Vec<Check>,
    pub(crate) steps: Vec<Step>,
    pub(crate) active: Option<usize>,
    #[serde(default, skip_serializing)]
    pub(crate) base: Option<Snapshot>,
    pub(crate) candidate: Option<Candidate>,
    pub(crate) pending: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) brief: Option<String>,
    #[serde(default)]
    pub(crate) deferred: u16,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) touched: BTreeSet<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) jobs: Vec<String>,
}

impl Work {
    pub(crate) fn new(objective: &str, policy: Policy) -> Self {
        let steps = match policy {
            Policy::Steps => Vec::new(),
            Policy::Single | Policy::Goal { .. } => vec![Step::new("Objective", None, Vec::new())],
        };
        Self {
            objective: objective.to_string(),
            policy,
            status: Status::Active(Stage::Plan),
            plan: None,
            checks: Vec::new(),
            steps,
            active: None,
            base: None,
            candidate: None,
            pending: Vec::new(),
            brief: None,
            deferred: 0,
            touched: BTreeSet::new(),
            jobs: Vec::new(),
        }
    }

    pub(crate) fn stage(&self) -> Option<Stage> {
        match &self.status {
            Status::Active(stage) => Some(*stage),
            Status::Completed | Status::Abandoned(_) | Status::Failed(_) => None,
        }
    }

    pub(crate) fn terminal(&self) -> bool {
        self.stage().is_none()
    }

    pub(crate) fn completion_checks(&self) -> Vec<Check> {
        if self.policy != Policy::Steps {
            return self.checks.clone();
        }
        self.active
            .and_then(|index| self.steps.get(index))
            .map(|step| {
                step.checks
                    .iter()
                    .filter_map(|index| self.checks.get(*index).cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn apply_todos(&mut self, items: &[(String, StepState)]) {
        if items.is_empty() {
            return;
        }
        let mut steps = Vec::new();
        for (title, state) in items {
            let mut step = self
                .steps
                .iter()
                .find(|step| step.title == *title)
                .cloned()
                .unwrap_or_else(|| Step::new(title, None, Vec::new()));
            step.state = *state;
            steps.push(step);
        }
        self.steps = steps;
        if self.policy == Policy::Single {
            self.policy = Policy::Steps;
        }
        self.active = self
            .steps
            .iter()
            .position(|step| step.state == StepState::Active)
            .or_else(|| {
                self.steps
                    .iter()
                    .position(|step| step.state == StepState::Pending)
            });
    }

    pub(crate) fn progress(&self) -> WorkProgress {
        let kind = match self.policy {
            Policy::Single => WorkKind::Single,
            Policy::Steps => WorkKind::Steps,
            Policy::Goal { .. } => WorkKind::Goal,
        };
        let (phase, reason) = match &self.status {
            Status::Active(Stage::Plan) => (WorkPhase::Plan, None),
            Status::Active(Stage::Execute) => (WorkPhase::Execute, None),
            Status::Active(Stage::Verify) => (WorkPhase::Verify, None),
            Status::Active(Stage::Review) => (WorkPhase::Review, None),
            Status::Completed => (WorkPhase::Completed, None),
            Status::Abandoned(reason) | Status::Failed(reason) => {
                (WorkPhase::Failed, Some(reason.clone()))
            }
        };
        let current = self.active.or_else(|| {
            self.steps
                .iter()
                .rposition(|step| matches!(step.state, StepState::Blocked | StepState::Completed))
        });
        let step = current.and_then(|index| self.steps.get(index));
        WorkProgress {
            kind,
            objective: self.objective.clone(),
            phase,
            checks_failed: matches!(self.status, Status::Failed(_)),
            current: (kind == WorkKind::Steps)
                .then(|| current.map(|index| index + 1))
                .flatten(),
            total: self.steps.len(),
            attempts: step.map_or(0, |step| step.attempts),
            rounds: step.map_or(0, |step| step.rounds),
            reason,
            ledger: self.ledger(),
            touched: self.touched.iter().cloned().collect(),
        }
    }

    fn planned_ledger(&self) -> Vec<String> {
        let Some(plan) = self.plan.as_deref() else {
            return Vec::new();
        };
        if let Ok(parsed) = crate::steps::parse(plan) {
            return parsed
                .iter()
                .take(LEDGER_MAX)
                .map(|step| format!("{}. {}", step.index, step.text))
                .collect();
        }
        plan.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .take(LEDGER_MAX)
            .map(str::to_string)
            .collect()
    }

    fn ledger(&self) -> Vec<String> {
        if self.policy == Policy::Single {
            return self.planned_ledger();
        }
        if self.policy != Policy::Steps {
            return Vec::new();
        }
        let mut lines = Vec::new();
        let mut phase: Option<&str> = None;
        let mut number = 0;
        let compact = self.steps.len() > LEDGER_MAX;
        for step in &self.steps {
            if step.phase.as_deref() != phase && step.phase.is_some() {
                phase = step.phase.as_deref();
                number += 1;
                let heading: String = phase
                    .unwrap_or_default()
                    .chars()
                    .take(LEDGER_WIDTH)
                    .collect();
                lines.push(format!("{}. {heading}", roman(number)));
            }
            if compact && step.state != StepState::Active {
                let tally = |lines: &mut Vec<String>, mark: &str, word: &str| {
                    let suffix = format!(" {word}");
                    match lines.last_mut() {
                        Some(line)
                            if line.starts_with(&format!("  {mark} "))
                                && line.ends_with(&suffix) =>
                        {
                            let count: usize = line
                                .split_whitespace()
                                .nth(2)
                                .and_then(|n| n.parse().ok())
                                .unwrap_or(1);
                            *line = format!("  {mark} {} {word}", count + 1);
                        }
                        _ => lines.push(format!("  {mark} 1 {word}")),
                    }
                };
                match step.state {
                    StepState::Completed => tally(&mut lines, "[x]", "done"),
                    StepState::Pending => tally(&mut lines, "[ ]", "later"),
                    StepState::Blocked => tally(&mut lines, "[!]", "blocked"),
                    StepState::Active => {}
                }
                continue;
            }
            let mark = match step.state {
                StepState::Pending => "[ ]",
                StepState::Active => "[/]",
                StepState::Completed => "[x]",
                StepState::Blocked => "[!]",
            };
            let indent = if phase.is_some() { "  " } else { "" };
            let title = crate::text::ellipsize(&step.title, LEDGER_WIDTH);
            lines.push(format!("{indent}{mark} {title}"));
        }
        lines
    }

    fn trees(&self) -> Trees<&Snapshot> {
        Trees {
            base: self.base.as_ref(),
            candidate: self.candidate.as_ref().map(|candidate| &candidate.snapshot),
        }
    }

    pub(crate) fn trees_mark(&self) -> u64 {
        let mut hasher = crate::text::Fnv64::default();
        self.trees().hash(&mut hasher);
        hasher.finish()
    }

    pub(crate) fn save(&self, path: &Path, trees: bool) -> io::Result<()> {
        let sidecar = path.with_extension(TREES);
        if trees && self.base.is_none() && self.candidate.is_none() {
            match std::fs::remove_file(&sidecar) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        } else if trees {
            store::write(&sidecar, serde_json::to_vec(&self.trees())?)?;
        }
        store::write(path, serde_json::to_vec(self)?)
    }

    pub(crate) fn load(path: &Path) -> io::Result<Option<Self>> {
        let mut work: Self = match std::fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let trees: Trees<Snapshot> = match std::fs::read(path.with_extension(TREES)) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Some(work)),
            Err(error) => return Err(error),
        };
        work.base = trees.base;
        if let Some(candidate) = &mut work.candidate {
            candidate.snapshot = trees.candidate.unwrap_or_default();
        }
        Ok(Some(work))
    }
}

#[cfg(test)]
#[path = "work_tests.rs"]
mod tests;
