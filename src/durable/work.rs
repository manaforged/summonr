use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::tree::{Snapshot, changed};

const REPAIR_LIMIT: u16 = 3;
const STALL_LIMIT: u16 = 3;
const DEFER_LIMIT: u16 = 3;
const LEDGER_WIDTH: usize = 72;
const LEDGER_MAX: usize = 40;
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
    Goal { max_turns: u32 },
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
    pub(crate) snapshot: Snapshot,
}

pub(crate) enum Decision {
    Automatic,
    GoalComplete,
    GoalContinue(String),
    GoalStop(String),
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
    pub(crate) base: Option<Snapshot>,
    pub(crate) candidate: Option<Candidate>,
    pub(crate) pending: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) brief: Option<String>,
    #[serde(default)]
    pub(crate) idle: u16,
    #[serde(default)]
    pub(crate) edited: bool,
    #[serde(default)]
    pub(crate) deferred: u16,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub(crate) touched: BTreeSet<String>,
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
            idle: 0,
            edited: false,
            deferred: 0,
            touched: BTreeSet::new(),
        }
    }

    pub(crate) fn stage(&self) -> Option<Stage> {
        match &self.status {
            Status::Active(stage) => Some(*stage),
            Status::Completed | Status::Abandoned(_) => None,
        }
    }

    pub(crate) fn terminal(&self) -> bool {
        self.stage().is_none()
    }

    pub(crate) fn select(&mut self, markdown: &str, commands: &[String]) -> Result<(), WorkError> {
        let checks = checks(markdown, self.policy, commands)?;
        if self.policy == Policy::Steps {
            let parsed = crate::steps::parse(markdown)?;
            let last = parsed.len().saturating_sub(1);
            self.steps = parsed
                .iter()
                .enumerate()
                .map(|(index, step)| {
                    let mut owned = Vec::new();
                    for command in commands.iter().chain(&step.checks) {
                        if let Some(position) = checks.iter().position(|check| {
                            check.observation.is_none() && check.command == *command
                        }) && !owned.contains(&position)
                        {
                            owned.push(position);
                        }
                    }
                    if index == last {
                        owned = (0..checks.len()).collect();
                    }
                    Step::new(&step.text, step.phase.as_deref(), owned)
                })
                .collect();
            self.active = None;
        }
        self.checks = checks;
        self.plan = Some(markdown.to_string());
        Ok(())
    }

    pub(crate) fn execute(&mut self, base: Snapshot) -> Result<(), WorkError> {
        if self.plan.is_none() {
            return Err(WorkError("Execute requires a selected plan".to_string()));
        }
        if let Some(first) = self.steps.first_mut() {
            first.state = StepState::Active;
        }
        self.active = (!self.steps.is_empty()).then_some(0);
        self.status = Status::Active(Stage::Execute);
        self.base = Some(base);
        Ok(())
    }

    pub(crate) fn verify(&mut self, capture: Snapshot) -> Result<Vec<String>, WorkError> {
        if self.stage() != Some(Stage::Execute) {
            return Err(WorkError("Verify requires Execute".to_string()));
        }
        let base = self
            .base
            .as_ref()
            .ok_or_else(|| WorkError("Execute has no workspace base".to_string()))?;
        let paths = changed(base, &capture);
        self.touched.extend(paths.iter().cloned());
        self.idle = if paths.is_empty() && !self.edited {
            self.idle.saturating_add(1)
        } else {
            0
        };
        self.edited = false;
        self.deferred = 0;
        self.candidate = Some(Candidate {
            paths: paths.clone(),
            snapshot: capture,
        });
        self.status = Status::Active(Stage::Verify);
        Ok(paths)
    }

    pub(crate) fn review(&mut self) -> Result<(), WorkError> {
        if self.stage() != Some(Stage::Verify) {
            return Err(WorkError("Review requires Verify".to_string()));
        }
        self.status = Status::Active(Stage::Review);
        Ok(())
    }

    pub(crate) fn verdict(&mut self, passed: bool, decision: Decision) -> Result<(), WorkError> {
        if !matches!(self.stage(), Some(Stage::Verify | Stage::Review)) {
            return Err(WorkError("a verdict requires Verify".to_string()));
        }
        if passed
            && matches!(
                (self.policy, &decision),
                (Policy::Goal { .. }, Decision::Automatic)
            )
        {
            return Err(WorkError(DECISION.to_string()));
        }
        if !passed {
            return self.fail("Repair the failed completion check.".to_string());
        }
        let active = self
            .active
            .ok_or_else(|| WorkError("Verify has no active item".to_string()))?;
        let policy = self.policy;
        let step = self
            .steps
            .get_mut(active)
            .ok_or_else(|| WorkError("active item is not in the plan".to_string()))?;
        step.rounds = step.rounds.saturating_add(1);
        step.attempts = 0;
        match (policy, decision) {
            (Policy::Single, _) | (Policy::Goal { .. }, Decision::GoalComplete) => {
                step.state = StepState::Completed;
                self.active = None;
                self.status = Status::Completed;
            }
            (Policy::Steps, _) => {
                step.state = StepState::Completed;
                match self
                    .steps
                    .iter()
                    .position(|step| step.state == StepState::Pending)
                {
                    Some(next) => {
                        if let Some(step) = self.steps.get_mut(next) {
                            step.state = StepState::Active;
                        }
                        self.active = Some(next);
                        self.status = Status::Active(Stage::Execute);
                        self.rebase();
                    }
                    None => {
                        self.active = None;
                        self.status = Status::Completed;
                    }
                }
            }
            (Policy::Goal { .. }, Decision::GoalStop(reason)) => {
                step.state = StepState::Blocked;
                self.active = None;
                self.status = Status::Abandoned(reason);
            }
            (Policy::Goal { max_turns }, Decision::GoalContinue(reason)) => {
                if self.idle >= STALL_LIMIT {
                    step.state = StepState::Blocked;
                    self.active = None;
                    self.status = Status::Abandoned(format!(
                        "stalled: no workspace change in {STALL_LIMIT} turns"
                    ));
                } else if step.rounds < max_turns {
                    step.state = StepState::Active;
                    self.status = Status::Active(Stage::Execute);
                    self.rebase();
                    self.pending.push(format!(
                        "[summon goal: the objective is not complete — {reason}. Continue with the next concrete action.]"
                    ));
                } else {
                    step.state = StepState::Blocked;
                    self.active = None;
                    self.status =
                        Status::Abandoned(format!("goal reached its {max_turns} turn limit"));
                }
            }
            (Policy::Goal { .. }, Decision::Automatic) => {
                return Err(WorkError(DECISION.to_string()));
            }
        }
        Ok(())
    }

    pub(crate) fn defer(&mut self, note: String) -> Result<bool, WorkError> {
        if self.stage() != Some(Stage::Execute) {
            return Err(WorkError("a deferral requires Execute".to_string()));
        }
        if self.deferred >= DEFER_LIMIT {
            return Ok(false);
        }
        self.deferred += 1;
        self.pending.push(note);
        Ok(true)
    }

    pub(crate) fn reject(&mut self, findings: &str) -> Result<(), WorkError> {
        if self.stage() != Some(Stage::Review) {
            return Err(WorkError("a rejection requires Review".to_string()));
        }
        self.fail(format!("Address the review findings:\n{findings}"))
    }

    fn fail(&mut self, continuation: String) -> Result<(), WorkError> {
        let active = self
            .active
            .ok_or_else(|| WorkError("Verify has no active item".to_string()))?;
        let policy = self.policy;
        let step = self
            .steps
            .get_mut(active)
            .ok_or_else(|| WorkError("active item is not in the plan".to_string()))?;
        step.rounds = step.rounds.saturating_add(1);
        step.attempts = step.attempts.saturating_add(1);
        if matches!(policy, Policy::Goal { .. }) && self.idle >= STALL_LIMIT {
            step.state = StepState::Blocked;
            self.active = None;
            self.status = Status::Abandoned(format!(
                "stalled: no workspace change in {STALL_LIMIT} turns"
            ));
        } else if policy != Policy::Single && step.attempts >= REPAIR_LIMIT {
            step.state = StepState::Blocked;
            self.active = None;
            self.status = Status::Abandoned("completion checks failed three times".to_string());
        } else {
            step.state = StepState::Active;
            self.status = Status::Active(Stage::Execute);
            self.rebase();
            self.pending.push(continuation);
        }
        Ok(())
    }

    pub(crate) fn mark_edit(&mut self) {
        self.edited = true;
    }

    fn rebase(&mut self) {
        if let Some(candidate) = self.candidate.take() {
            self.base = Some(candidate.snapshot);
        }
    }

    pub(crate) fn abandon(&mut self, reason: &str) {
        if self.terminal() {
            return;
        }
        if let Some(step) = self.active.and_then(|index| self.steps.get_mut(index)) {
            step.state = StepState::Blocked;
        }
        self.active = None;
        self.status = Status::Abandoned(reason.to_string());
    }

    pub(crate) fn interrupt(&mut self) -> bool {
        if !matches!(self.stage(), Some(Stage::Verify | Stage::Review)) {
            return false;
        }
        self.status = Status::Active(Stage::Execute);
        self.candidate = None;
        true
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

    pub(crate) fn progress(&self) -> WorkProgress {
        let (kind, max_turns) = match self.policy {
            Policy::Single => (WorkKind::Single, 0),
            Policy::Steps => (WorkKind::Steps, 0),
            Policy::Goal { max_turns } => (WorkKind::Goal, max_turns),
        };
        let (phase, reason) = match &self.status {
            Status::Active(Stage::Plan) => (WorkPhase::Plan, None),
            Status::Active(Stage::Execute) => (WorkPhase::Execute, None),
            Status::Active(Stage::Verify) => (WorkPhase::Verify, None),
            Status::Active(Stage::Review) => (WorkPhase::Review, None),
            Status::Completed => (WorkPhase::Completed, None),
            Status::Abandoned(reason) => (WorkPhase::Failed, Some(reason.clone())),
        };
        let current = self.active.or_else(|| {
            self.steps
                .iter()
                .rposition(|step| matches!(step.state, StepState::Blocked | StepState::Completed))
        });
        let step = current.and_then(|index| self.steps.get(index));
        WorkProgress {
            kind,
            phase,
            current: (kind == WorkKind::Steps)
                .then(|| current.map(|index| index + 1))
                .flatten(),
            total: self.steps.len(),
            attempts: step.map_or(0, |step| step.attempts),
            rounds: step.map_or(0, |step| step.rounds),
            max_turns,
            reason,
            ledger: self.ledger(),
            touched: self.touched.iter().cloned().collect(),
        }
    }

    fn ledger(&self) -> Vec<String> {
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
            let title: String = step.title.chars().take(LEDGER_WIDTH).collect();
            let ellipsis = if step.title.chars().count() > LEDGER_WIDTH {
                "…"
            } else {
                ""
            };
            lines.push(format!("{indent}{mark} {title}{ellipsis}"));
        }
        lines
    }

    pub(crate) fn save(&self, path: &Path) -> io::Result<()> {
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(temp, path)
    }

    pub(crate) fn load(path: &Path) -> io::Result<Option<Self>> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
}

pub(crate) fn roman(number: usize) -> String {
    const DIGITS: [(usize, &str); 13] = [
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut rest = number;
    let mut text = String::new();
    for (value, glyph) in DIGITS {
        while rest >= value {
            text.push_str(glyph);
            rest -= value;
        }
    }
    text
}

pub(crate) fn checks(
    markdown: &str,
    policy: Policy,
    commands: &[String],
) -> Result<Vec<Check>, WorkError> {
    let mut checks: Vec<Check> = commands
        .iter()
        .map(|command| Check {
            command: command.clone(),
            observation: None,
        })
        .collect();
    if policy != Policy::Steps {
        return Ok(checks);
    }
    let mut push = |command: String, observation: Option<Observation>| {
        if !checks
            .iter()
            .any(|known| known.command == command && known.observation == observation)
        {
            checks.push(Check {
                command,
                observation,
            });
        }
    };
    for command in crate::steps::parse(markdown)?
        .iter()
        .flat_map(|step| &step.checks)
    {
        push(command.clone(), None);
    }
    for item in crate::steps::contract(markdown)?.items {
        match item {
            crate::steps::Item::File(path) => {
                push(
                    format!("file exists: {path}"),
                    Some(Observation::File(path)),
                );
            }
            crate::steps::Item::Stdout { command, expected } => {
                push(command, Some(Observation::Stdout(expected)));
            }
        }
    }
    Ok(checks)
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    Single,
    Steps,
    Goal,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkPhase {
    Plan,
    Execute,
    Verify,
    Review,
    Completed,
    Failed,
}

impl Display for WorkPhase {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Plan => "planning",
            Self::Execute => "running",
            Self::Verify => "verifying",
            Self::Review => "reviewing",
            Self::Completed => "completed",
            Self::Failed => "failed",
        })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct WorkProgress {
    pub kind: WorkKind,
    pub phase: WorkPhase,
    pub current: Option<usize>,
    pub total: usize,
    pub attempts: u16,
    pub rounds: u32,
    pub max_turns: u32,
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ledger: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub touched: Vec<String>,
}

impl WorkProgress {
    #[must_use]
    pub fn terminal(&self) -> bool {
        matches!(self.phase, WorkPhase::Completed | WorkPhase::Failed)
    }

    #[must_use]
    pub fn exit_code(&self) -> Option<i32> {
        match self.phase {
            WorkPhase::Completed => Some(0),
            WorkPhase::Failed
                if self
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.contains("completion checks")) =>
            {
                Some(2)
            }
            WorkPhase::Failed => Some(3),
            WorkPhase::Plan | WorkPhase::Execute | WorkPhase::Verify | WorkPhase::Review => None,
        }
    }
}

impl Display for WorkProgress {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match (self.kind, self.current) {
            (WorkKind::Steps, Some(current)) => {
                write!(
                    formatter,
                    "Work step {current}/{} · {}",
                    self.total, self.phase
                )?;
            }
            (WorkKind::Goal, _) => {
                if self.max_turns > 0 {
                    write!(
                        formatter,
                        "Work goal turn {}/{} · {}",
                        self.rounds, self.max_turns, self.phase
                    )?;
                } else {
                    write!(formatter, "Work goal turn {} · {}", self.rounds, self.phase)?;
                }
            }
            _ => write!(formatter, "Work · {}", self.phase)?,
        }
        if let Some(reason) = &self.reason {
            write!(formatter, " · {reason}")?;
        }
        Ok(())
    }
}
