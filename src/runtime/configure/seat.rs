use super::*;

impl<P: Provider> Loop<P> {
    pub(crate) fn set_provider(&mut self, provider: P, model: impl Into<String>) {
        if let Some(probe) = self.probe.take() {
            probe.abort();
        }
        self.provider = provider;
        self.model = model.into();
        self.context_sample = None;
    }

    pub fn set_compact_tokens(&mut self, tokens: Option<u32>) {
        self.compact_tokens = tokens;
    }

    pub fn set_fallbacks(&mut self, fallbacks: impl IntoIterator<Item = (P, String)>) {
        self.fallbacks = fallbacks.into_iter().collect();
    }

    pub(crate) fn set_seat(
        &mut self,
        role: workflow::Role,
        provider: P,
        model: impl Into<String>,
        effort: Option<crate::provider::Effort>,
        price: Option<crate::provider::Price>,
    ) {
        self.seats.insert(role, (provider, model.into()));
        self.tuning.insert(role, (effort, price));
    }

    pub(crate) fn clear_seat(&mut self, role: workflow::Role) {
        self.seats.remove(&role);
        self.tuning.remove(&role);
    }

    pub fn set_price(&mut self, price: Option<crate::provider::Price>) {
        self.price = price;
    }

    #[must_use]
    pub(crate) fn seat_model(&self, role: workflow::Role) -> Option<&str> {
        self.seats.get(&role).map(|(_, model)| model.as_str())
    }

    pub(crate) fn set_workflow(&mut self, name: Option<&str>) -> Result<()> {
        self.session.set_workflow(name).map_err(super::session_err)
    }

    pub fn apply_workflow(
        &mut self,
        workflow: &workflow::Workflow,
        mut resolve: impl FnMut(&workflow::Seat) -> Result<(P, crate::endpoint::Endpoint)>,
    ) -> Result<()> {
        let mut seats = BTreeMap::new();
        let mut tuning = BTreeMap::new();
        for (role, seat) in &workflow.seats {
            let (provider, endpoint) = resolve(seat).map_err(|error| {
                Error::Config(format!("{}: {role} seat {seat}: {error}", workflow.name))
            })?;
            seats.insert(*role, (provider, endpoint.model));
            tuning.insert(*role, (endpoint.effort, endpoint.price));
        }
        self.set_workflow(Some(&workflow.name))?;
        self.seats = seats;
        self.tuning = tuning;
        Ok(())
    }

    #[must_use]
    pub fn workflow(&self) -> Option<String> {
        self.session.workflow().map(str::to_string)
    }

    pub(crate) fn set_verify(&mut self, commands: Option<Vec<String>>) {
        self.verify = commands;
    }

    #[must_use]
    pub fn with_verify(mut self, commands: Vec<String>) -> Self {
        self.set_verify(Some(commands));
        self
    }

    pub fn with_budget(mut self, budget: Budget) -> Result<Self> {
        self.session
            .set_budget(
                budget.max_tokens,
                budget.max_secs.map(|seconds| seconds.saturating_mul(1_000)),
            )
            .map_err(super::session_err)?;
        self.budget = budget;
        Ok(self)
    }

    #[must_use]
    pub fn history_len(&self) -> usize {
        self.session.history().len()
    }

    #[must_use]
    pub fn session_path(&self) -> &Path {
        self.session.path()
    }

    pub fn set_name(&self, name: &str) -> Result<()> {
        crate::session::set_name(self.session.path(), name)?;
        Ok(())
    }

    #[must_use]
    pub fn name(&self) -> Option<String> {
        crate::session::catalog_name(self.session.path())
    }

    #[must_use]
    pub fn session_id(&self) -> String {
        self.session.id().to_string()
    }

    pub fn work_progress(&self) -> Result<crate::WorkProgress> {
        self.session.work_progress().map_err(super::session_err)
    }

    pub fn next_step(
        &self,
        steps: &[crate::steps::Step],
        plan_path: &str,
    ) -> Result<crate::StepRun> {
        let progress = self.work_progress()?;
        if let Some(code) = progress.exit_code() {
            return Ok(crate::StepRun::Done(code));
        }
        let current = progress
            .current
            .ok_or_else(|| Error::Config("Steps Work has no active item".to_string()))?;
        let step = steps
            .get(current.saturating_sub(1))
            .ok_or_else(|| Error::Config("Steps Work is outside the selected plan".to_string()))?;
        Ok(crate::StepRun::Run {
            index: current,
            total: steps.len(),
            prompt: crate::steps::prompt(step, steps.len(), plan_path),
        })
    }
}
