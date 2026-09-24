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

    pub(crate) fn needs_turn_plan(&self) -> bool {
        self.active().is_some_and(|work| {
            work.policy == Policy::Single
                && work.stage() == Some(Stage::Plan)
                && work.plan.is_none()
        })
    }

    #[must_use]
    pub(crate) fn has_selected_plan(&self) -> bool {
        self.work.as_ref().is_some_and(|work| {
            work.plan
                .as_ref()
                .is_some_and(|text| !text.trim().is_empty())
        }) || crate::plan::read(&self.workspace).is_some()
    }

    pub(crate) fn select_plan(
        &mut self,
        markdown: &str,
        commands: &[String],
    ) -> Result<(), DriverError> {
        let work = self.work.as_mut().ok_or(DriverError::NoWork)?;
        if work.policy != Policy::Single || work.stage() != Some(Stage::Plan) {
            return Err(DriverError::State(
                "no single-policy plan stage is open".to_string(),
            ));
        }
        work.select(markdown, commands)?;
        self.checkpoint()?;
        self.execute()
    }

    pub(crate) fn begin_without_plan(&mut self, commands: &[String]) -> Result<(), DriverError> {
        let objective = self
            .work
            .as_ref()
            .ok_or(DriverError::NoWork)?
            .objective
            .clone();
        let work = self.work.as_mut().ok_or(DriverError::NoWork)?;
        if work.policy != Policy::Single || work.stage() != Some(Stage::Plan) || work.plan.is_some()
        {
            return Ok(());
        }
        work.select(&objective, commands)?;
        self.checkpoint()?;
        self.execute()
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

    pub(crate) fn prepare_plan_work(&mut self, objective: &str) -> Result<(), DriverError> {
        match self.active() {
            None => self.set_work(objective, Policy::Single),
            Some(work) if work.policy == Policy::Single && work.stage() == Some(Stage::Plan) => {
                Ok(())
            }
            Some(_) => Err(DriverError::State(
                "another Work is already active".to_string(),
            )),
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "one record for a finished plan round"
    )]
    pub(crate) fn finish_plan(
        &mut self,
        attempt: Attempt,
        markdown: &str,
        reasoning: &str,
        usage: Usage,
        meta: UsageMeta,
        commands: &[String],
        receipt: &crate::verify::Receipt,
    ) -> Result<(), DriverError> {
        let round = self.end_round(attempt)?;
        self.answer(round, markdown, reasoning, usage, &[], meta)?;
        self.work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .select(markdown, commands)?;
        self.checkpoint()?;
        crate::plan::write(&self.workspace, markdown)?;
        self.append(Line::Receipt {
            text: String::new(),
            receipt: Some(receipt.clone()),
        })?;
        self.close()
    }

    pub(crate) fn accept_plan(&mut self) -> Result<(), DriverError> {
        self.pending_plan()?;
        if let Some(text) = crate::plan::read(&self.workspace) {
            let commands: Vec<String> = self
                .work
                .as_ref()
                .ok_or(DriverError::NoWork)?
                .checks
                .iter()
                .filter(|check| check.observation.is_none())
                .map(|check| check.command.clone())
                .collect();
            self.work
                .as_mut()
                .ok_or(DriverError::NoWork)?
                .select(&text, &commands)?;
            self.checkpoint()?;
        }
        self.execute()
    }

    pub(crate) fn reject_plan(&mut self) -> Result<(), DriverError> {
        self.pending_plan()?;
        self.abandon_work("plan rejected by the user")
    }

    pub(crate) fn pending_plan(&self) -> Result<(), DriverError> {
        let selected = self.active().is_some_and(|work| {
            work.policy == Policy::Single
                && work.stage() == Some(Stage::Plan)
                && work.plan.is_some()
        });
        if !selected {
            return Err(DriverError::State(
                "No pending plan. Use /auto to leave plan mode, or /plan and a request to create a plan.".to_string(),
            ));
        }
        Ok(())
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
        match self.active() {
            Some(work)
                if std::mem::discriminant(&work.policy) != std::mem::discriminant(&policy) =>
            {
                return Err(DriverError::State(
                    "another Work policy is already active".to_string(),
                ));
            }
            Some(work) if work.objective != text => {
                return Err(DriverError::State(
                    "another Work objective is already active".to_string(),
                ));
            }
            Some(_) => {}
            None => self.set_work(text, policy)?,
        }
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
        match work.stage() {
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
        if created {
            self.set_work(objective, Policy::Single)?;
            if plan_first {
                self.work
                    .as_mut()
                    .ok_or(DriverError::NoWork)?
                    .set_checks(commands)?;
                self.checkpoint()?;
                return Ok(TurnWork::NeedsPlan);
            }
            self.work
                .as_mut()
                .ok_or(DriverError::NoWork)?
                .select(objective, commands)?;
        }
        let work = self.work.as_mut().ok_or(DriverError::NoWork)?;
        if plan_first && work.policy == Policy::Single && work.plan.is_none() {
            work.set_checks(commands)?;
            self.checkpoint()?;
            return Ok(TurnWork::NeedsPlan);
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
        let work = self.work.as_ref().ok_or(DriverError::NoWork)?;
        match work.stage() {
            Some(Stage::Plan) if work.policy == Policy::Single && work.plan.is_none() => {
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

    fn active(&self) -> Option<&Work> {
        self.work.as_ref().filter(|work| !work.terminal())
    }

    fn set_work(&mut self, objective: &str, policy: Policy) -> Result<(), DriverError> {
        self.work = Some(Work::new(objective, policy));
        self.checkpoint()
    }
}
