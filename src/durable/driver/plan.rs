use super::*;

impl SessionDriver {
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
        }) || crate::plan::read(self.log.path()).is_some()
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
        crate::plan::write(self.log.path(), markdown)?;
        self.append(Line::Receipt {
            text: String::new(),
            receipt: Some(receipt.clone()),
        })?;
        self.close()
    }

    pub(crate) fn accept_plan(&mut self) -> Result<(), DriverError> {
        self.pending_plan()?;
        if let Some(text) = crate::plan::read(self.log.path()) {
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

    pub(crate) fn leave_plan(&mut self) -> Result<Option<String>, DriverError> {
        let Some(work) = self
            .active()
            .filter(|work| work.policy == Policy::Single && work.stage() == Some(Stage::Plan))
        else {
            return Ok(None);
        };
        let plan = work.plan.clone();
        self.abandon_work("left plan mode")?;
        Ok(plan)
    }
}
