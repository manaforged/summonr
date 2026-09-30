use super::*;

impl Work {
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

    pub(crate) fn set_checks(&mut self, commands: &[String]) -> Result<(), WorkError> {
        self.checks = checks(&self.objective, self.policy, commands)?;
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
        self.deferred = 0;
        self.jobs.clear();
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
        let active = match self.active.or_else(|| {
            self.steps
                .iter()
                .position(|step| step.state == StepState::Active)
                .or_else(|| {
                    self.steps
                        .iter()
                        .position(|step| step.state == StepState::Pending)
                })
        }) {
            Some(active) => active,
            None => {
                self.status = Status::Completed;
                return Ok(());
            }
        };
        self.active = Some(active);
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
            (Policy::Goal { .. }, Decision::GoalContinue(reason)) => {
                step.state = StepState::Active;
                self.status = Status::Active(Stage::Execute);
                self.rebase();
                self.pending.push(format!(
                    "[summon goal: the objective is not complete — {reason}]"
                ));
            }
            (Policy::Goal { .. }, Decision::Automatic) => {
                return Err(WorkError(DECISION.to_string()));
            }
        }
        Ok(())
    }

    pub(crate) fn defer(&mut self, note: String, jobs: Vec<String>) -> Result<bool, WorkError> {
        if self.stage() != Some(Stage::Execute) {
            return Err(WorkError("a deferral requires Execute".to_string()));
        }
        self.deferred = self.deferred.saturating_add(1);
        self.pending.push(note);
        self.jobs = jobs;
        Ok(true)
    }

    pub(crate) fn reject(&mut self, findings: &str) -> Result<(), WorkError> {
        if self.stage() != Some(Stage::Review) {
            return Err(WorkError("a rejection requires Review".to_string()));
        }
        self.fail(format!("Address the review findings:\n{findings}"))
    }

    fn fail(&mut self, continuation: String) -> Result<(), WorkError> {
        let active = match self.active.or_else(|| {
            self.steps
                .iter()
                .position(|step| step.state != StepState::Completed)
        }) {
            Some(active) => active,
            None => {
                self.status = Status::Failed(continuation);
                return Ok(());
            }
        };
        self.active = Some(active);
        let step = self
            .steps
            .get_mut(active)
            .ok_or_else(|| WorkError("active item is not in the plan".to_string()))?;
        step.rounds = step.rounds.saturating_add(1);
        step.attempts = step.attempts.saturating_add(1);
        step.state = StepState::Active;
        self.status = Status::Active(Stage::Execute);
        self.rebase();
        self.pending.push(continuation);
        Ok(())
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
}
