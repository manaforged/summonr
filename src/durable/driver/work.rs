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
        items: &[(String, crate::durable::work::StepState)],
    ) -> Result<(), DriverError> {
        let work = self.work.as_mut().ok_or(DriverError::NoWork)?;
        work.apply_todos(items);
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

    pub(crate) fn needs_scout(&self, plan: bool) -> bool {
        self.active().is_some_and(|work| {
            work.brief.is_none()
                && match work.policy {
                    Policy::Steps | Policy::Goal { .. } => true,
                    Policy::Single => plan,
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
                return Err(DriverError::State(if work.policy == Policy::Single {
                    "a plan is pending; /plan accept runs it, /plan reject drops it".to_string()
                } else {
                    "another Work is active; /goal clear or /steps clear ends it".to_string()
                }));
            }
            Some(work) if work.objective != text => {
                return Err(DriverError::State(
                    "another Work objective is already active".to_string(),
                ));
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
        plan_first: bool,
    ) -> Result<TurnWork, DriverError> {
        let created = self.active().is_none();
        if created && let Some(turn) = self.create_turn_work(objective, commands, plan_first)? {
            return Ok(turn);
        }
        if let Some(turn) = self.sync_turn_checks(commands, plan_first)? {
            return Ok(turn);
        }
        self.advance_turn_work(objective, commands, created)
    }

    fn create_turn_work(
        &mut self,
        objective: &str,
        commands: &[String],
        plan_first: bool,
    ) -> Result<Option<TurnWork>, DriverError> {
        self.set_work(objective, Policy::Single)?;
        if plan_first {
            self.work
                .as_mut()
                .ok_or(DriverError::NoWork)?
                .set_checks(commands)?;
            self.checkpoint()?;
            return Ok(Some(TurnWork::NeedsPlan));
        }
        self.work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .select(objective, commands)?;
        Ok(None)
    }

    fn sync_turn_checks(
        &mut self,
        commands: &[String],
        plan_first: bool,
    ) -> Result<Option<TurnWork>, DriverError> {
        let work = self.work.as_mut().ok_or(DriverError::NoWork)?;
        if plan_first && work.policy == Policy::Single && work.plan.is_none() {
            work.set_checks(commands)?;
            self.checkpoint()?;
            return Ok(Some(TurnWork::NeedsPlan));
        }
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
        Ok(None)
    }

    fn advance_turn_work(
        &mut self,
        objective: &str,
        commands: &[String],
        created: bool,
    ) -> Result<TurnWork, DriverError> {
        let work = self.work.as_ref().ok_or(DriverError::NoWork)?;
        match work.stage() {
            Some(Stage::Plan) if work.policy == Policy::Single && work.plan.is_none() => {
                self.replan_turn_work(objective, commands)
            }
            Some(Stage::Plan) => {
                self.execute()?;
                Ok(TurnWork::Ready { accepted: !created })
            }
            Some(Stage::Execute) => Ok(TurnWork::Ready { accepted: false }),
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
    ) -> Result<TurnWork, DriverError> {
        if let Some(work) = self.work.as_mut() {
            work.abandon("the plan turn selected no plan");
        }
        self.set_work(objective, Policy::Single)?;
        self.work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .select(objective, commands)?;
        self.execute()?;
        Ok(TurnWork::Ready { accepted: false })
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
