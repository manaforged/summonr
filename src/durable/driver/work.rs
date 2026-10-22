use super::*;

impl SessionDriver {
    pub(crate) fn defer(&mut self, note: &str, jobs: Vec<String>) -> Result<bool, DriverError> {
        if self.turn.is_none() {
            return Err(DriverError::NoTurn);
        }
        let deferred = self
            .work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .defer(note.to_string(), jobs)?;
        if deferred {
            self.checkpoint()?;
            self.close()?;
        }
        Ok(deferred)
    }

    pub(crate) fn jobs(&self) -> Vec<String> {
        self.active()
            .map(|work| work.jobs.clone())
            .unwrap_or_default()
    }

    pub(crate) fn work_progress(&self) -> Result<WorkProgress, DriverError> {
        Ok(self.work.as_ref().ok_or(DriverError::NoWork)?.progress())
    }

    pub(crate) fn apply_todos(
        &mut self,
        todos: Vec<crate::tools::Todo>,
    ) -> Result<(), DriverError> {
        let work = self
            .work
            .as_mut()
            .filter(|work| !work.terminal())
            .ok_or(DriverError::NoWork)?;
        work.apply_todos(todos);
        self.checkpoint()?;
        Ok(())
    }

    pub(crate) fn abandon_work(&mut self, reason: &str) -> Result<(), DriverError> {
        self.work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .abandon(reason);
        self.checkpoint()
    }

    pub(crate) fn objective(&self) -> Result<String, DriverError> {
        Ok(self
            .work
            .as_ref()
            .ok_or(DriverError::NoWork)?
            .objective
            .clone())
    }

    pub(crate) fn needs_scout(&self) -> bool {
        self.active().is_some_and(|work| {
            work.brief.is_none()
                && match work.policy {
                    Policy::Steps | Policy::Goal { .. } => true,
                    Policy::Single => false,
                }
        })
    }

    pub(crate) fn set_brief(&mut self, brief: &str) -> Result<(), DriverError> {
        self.work.as_mut().ok_or(DriverError::NoWork)?.brief = Some(brief.to_string());
        self.checkpoint()
    }

    pub(crate) fn goal_objective(&self) -> Result<Option<String>, DriverError> {
        let work = self.work.as_ref().ok_or(DriverError::NoWork)?;
        Ok(matches!(work.policy, Policy::Goal { .. }).then(|| work.objective.clone()))
    }

    pub(crate) fn completion_checks(&self) -> Result<Vec<Check>, DriverError> {
        Ok(self
            .work
            .as_ref()
            .ok_or(DriverError::NoWork)?
            .completion_checks())
    }

    pub(crate) fn prepare_external_work(
        &mut self,
        text: &str,
        policy: Policy,
        commands: &[String],
    ) -> Result<(), DriverError> {
        if policy == Policy::Single {
            return Err(DriverError::State(
                "external Work requires Steps or Goal policy".to_string(),
            ));
        }
        self.adopt_external_work(text, policy)?;
        let work = self.work.as_mut().ok_or(DriverError::NoWork)?;
        if work.plan.is_none() {
            work.select(text, commands)?;
        }
        let expected = crate::durable::work::checks(text, policy, commands)?;
        if work.plan.as_deref() != Some(text) || work.checks != expected {
            return Err(DriverError::State(
                "the plan or completion checks changed while Work was active".to_string(),
            ));
        }
        self.resume_external_work()
    }

    fn adopt_external_work(&mut self, text: &str, policy: Policy) -> Result<(), DriverError> {
        match self.active() {
            Some(work)
                if std::mem::discriminant(&work.policy) != std::mem::discriminant(&policy) =>
            {
                return Err(DriverError::State(
                    "another Work is active; /goal clear or /steps clear ends it".to_string(),
                ));
            }
            Some(work) if work.objective != text => {
                return Err(DriverError::State(format!(
                    "`{}` is still active; /goal clear or /steps clear ends it",
                    work.objective
                )));
            }
            Some(_) => {}
            None => self.set_work(text, policy)?,
        }
        Ok(())
    }

    fn resume_external_work(&mut self) -> Result<(), DriverError> {
        let stage = self.work.as_ref().ok_or(DriverError::NoWork)?.stage();
        match stage {
            Some(Stage::Plan) => {
                self.checkpoint()?;
                self.execute()
            }
            Some(Stage::Execute) => self.checkpoint(),
            Some(Stage::Verify | Stage::Review) => Err(DriverError::State(
                "Work is still in Verify after an interrupted turn".to_string(),
            )),
            None => Err(DriverError::NoWork),
        }
    }

    pub(crate) fn prepare_turn_work(
        &mut self,
        objective: &str,
        commands: &[String],
    ) -> Result<(), DriverError> {
        if self.active().is_none() {
            let todos = self
                .work
                .as_ref()
                .map(|work| work.todos.clone())
                .unwrap_or_default();
            let mut work = Work::new(objective, Policy::Single);
            work.select(objective, commands)?;
            work.apply_todos(todos);
            self.work = Some(work);
            self.checkpoint()?;
        }
        self.sync_turn_checks(commands)?;
        self.advance_turn_work(objective, commands)
    }

    fn sync_turn_checks(&mut self, commands: &[String]) -> Result<(), DriverError> {
        let work = self.work.as_mut().ok_or(DriverError::NoWork)?;
        if work.policy == Policy::Single
            && work
                .checks
                .iter()
                .map(|check| check.command.as_str())
                .ne(commands.iter().map(String::as_str))
        {
            let plan = work.plan.clone().unwrap_or_else(|| work.objective.clone());
            work.select(&plan, commands)?;
            self.checkpoint()?;
        }
        Ok(())
    }

    fn advance_turn_work(
        &mut self,
        objective: &str,
        commands: &[String],
    ) -> Result<(), DriverError> {
        let work = self.work.as_ref().ok_or(DriverError::NoWork)?;
        match work.stage() {
            Some(Stage::Plan) if work.policy == Policy::Single && work.plan.is_none() => {
                self.replan_turn_work(objective, commands)
            }
            Some(Stage::Plan) => self.execute(),
            Some(Stage::Execute) => Ok(()),
            Some(Stage::Verify | Stage::Review) => Err(DriverError::State(
                "Work is still in Verify after an interrupted turn".to_string(),
            )),
            None => Err(DriverError::NoWork),
        }
    }

    fn replan_turn_work(
        &mut self,
        objective: &str,
        commands: &[String],
    ) -> Result<(), DriverError> {
        if let Some(work) = self.work.as_mut() {
            work.abandon("the plan turn selected no plan");
        }
        self.set_work(objective, Policy::Single)?;
        self.work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .select(objective, commands)?;
        self.execute()
    }

    pub(crate) fn record_check(&mut self, passed: bool) -> Result<(), DriverError> {
        self.work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .record_check(passed);
        self.checkpoint()
    }

    pub(super) fn active(&self) -> Option<&Work> {
        self.work.as_ref().filter(|work| !work.terminal())
    }

    pub(super) fn set_work(&mut self, objective: &str, policy: Policy) -> Result<(), DriverError> {
        self.work = Some(Work::new(objective, policy));
        self.checkpoint()
    }
}
