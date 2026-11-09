use std::collections::{BTreeMap, BTreeSet};

use std::hash::{Hash, Hasher};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::log::{self, Line};
use super::store;
use super::tree::{Entry, Snapshot, changed};

mod ledger;
mod progress;
mod stage;

pub(crate) use progress::{checks, roman};
pub(crate) use summonr_types::WorkPhase;
pub use summonr_types::{WorkKind, WorkProgress};

const LEDGER_WIDTH: usize = 72;
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

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CheckVerdict {
    Passed,
    Failed,
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
pub(crate) struct Measurement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) value: Option<String>,
    pub(crate) kept: bool,
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

#[derive(Deserialize)]
#[serde(transparent)]
struct LegacyFiles(BTreeMap<String, Entry>);

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Work {
    pub(crate) objective: String,
    #[serde(
        serialize_with = "progress::persist",
        deserialize_with = "progress::restore"
    )]
    pub(crate) policy: WorkKind,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) check: Option<CheckVerdict>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) failed_checks: Option<Vec<crate::verify::Outcome>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) todos: Vec<summonr_types::Todo>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) measurements: Vec<Measurement>,
}

impl Work {
    pub(crate) fn new(objective: &str, policy: WorkKind) -> Self {
        let steps = match policy {
            WorkKind::Steps => Vec::new(),
            WorkKind::Single | WorkKind::Goal => vec![Step::new("Objective", None, Vec::new())],
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
            check: None,
            failed_checks: None,
            todos: Vec::new(),
            measurements: Vec::new(),
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
        if self.policy != WorkKind::Steps {
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

    pub(crate) fn record_check(&mut self, passed: bool) {
        self.check = Some(if passed {
            CheckVerdict::Passed
        } else {
            CheckVerdict::Failed
        });
    }

    pub(crate) fn best(&self) -> Option<f64> {
        self.measurements
            .iter()
            .rev()
            .filter(|measurement| measurement.kept)
            .find_map(|measurement| measurement.value.as_deref()?.parse().ok())
    }

    pub(crate) fn measure(&mut self, value: Option<String>, kept: bool) {
        self.measurements.push(Measurement { value, kept });
    }

    pub(crate) fn apply_todos(&mut self, todos: Vec<summonr_types::Todo>) {
        self.todos = todos;
    }

    pub(crate) fn progress(&self) -> WorkProgress {
        let kind = self.policy;
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
            checks_failed: self.check == Some(CheckVerdict::Failed),
            current: (kind == WorkKind::Steps)
                .then(|| current.map(|index| index + 1))
                .flatten(),
            total: self.steps.len(),
            attempts: step.map_or(0, |step| step.attempts),
            rounds: step.map_or(0, |step| step.rounds),
            reason,
            ledger: self.ledger(),
            todos: self.todos.clone(),
            touched: self.touched.iter().cloned().collect(),
        }
    }

    fn trees(&self) -> Trees<&Snapshot> {
        Trees {
            base: self.base.as_ref(),
            candidate: self.candidate.as_ref().map(|candidate| &candidate.snapshot),
        }
    }

    pub(crate) fn trees_mark(&self) -> u64 {
        let mut hasher = summonr_core::text::Fnv64::default();
        self.trees().hash(&mut hasher);
        hasher.finish()
    }

    pub(crate) fn durability(&self) -> store::Durability {
        match self.policy {
            WorkKind::Single => store::Durability::Renamed,
            WorkKind::Steps | WorkKind::Goal => store::Durability::Synced,
        }
    }

    pub(crate) fn save_trees(&self, session: &Path) -> io::Result<()> {
        let sidecar = trees_path(session);
        if self.base.is_none() && self.candidate.is_none() {
            return match std::fs::remove_file(&sidecar) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
                _ => Ok(()),
            };
        }
        store::write(
            &sidecar,
            serde_json::to_vec(&self.trees())?,
            self.durability(),
        )
    }

    pub(crate) fn at(session: &Path) -> Option<Self> {
        Self::restore(session, last_logged(session)).ok()?.ok()
    }

    pub(crate) fn restore(
        session: &Path,
        logged: Option<Self>,
    ) -> io::Result<Result<Self, Option<String>>> {
        let mut work = match logged {
            Some(work) => work,
            None => match legacy(&log::work_path(session))? {
                Ok(work) => work,
                Err(notice) => return Ok(Err(notice)),
            },
        };
        work.repair_steps();
        let Some(trees) = read_trees(&trees_path(session))? else {
            return Ok(Ok(work));
        };
        work.base = trees.base;
        if let Some(candidate) = &mut work.candidate {
            candidate.snapshot = trees.candidate.unwrap_or_default();
        }
        Ok(Ok(work))
    }
}

fn last_logged(session: &Path) -> Option<Work> {
    const MARK: &[u8] = br#""kind":"work""#;
    let bytes = std::fs::read(session).ok()?;
    bytes
        .rsplit(|byte| *byte == b'\n')
        .filter(|line| line.windows(MARK.len()).any(|window| window == MARK))
        .find_map(|line| match serde_json::from_slice(line) {
            Ok(Line::Work { work }) => Some(*work),
            _ => None,
        })
}

fn legacy(path: &Path) -> io::Result<Result<Work, Option<String>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).map_err(|error| {
            Some(format!(
                "{} is unreadable ({error}); the session starts without its saved Work",
                path.display()
            ))
        })),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Err(None)),
        Err(error) => Err(error),
    }
}

fn trees_path(session: &Path) -> PathBuf {
    log::work_path(session).with_extension(TREES)
}

fn read_trees(path: &Path) -> io::Result<Option<Trees<Snapshot>>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if let Ok(legacy) = serde_json::from_slice::<Trees<LegacyFiles>>(&bytes) {
        return Ok(Some(Trees {
            base: legacy.base.map(|files| Snapshot::from_files(files.0)),
            candidate: legacy.candidate.map(|files| Snapshot::from_files(files.0)),
        }));
    }
    Ok(serde_json::from_slice(&bytes).ok())
}

#[cfg(test)]
#[path = "work_tests.rs"]
mod tests;
