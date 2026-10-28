use super::*;

impl<P: Provider> Loop<P> {
    pub(crate) fn set_provider(&mut self, provider: P, model: impl Into<String>) {
        self.probes.retain(|(role, probe)| {
            let keep = role.is_some();
            if !keep {
                probe.abort();
            }
            keep
        });
        self.tools.set_model_images(provider.takes_images());
        self.provider = provider;
        self.model = model.into();
        self.herdr.set_model(&self.model);
        self.context_sample = None;
    }

    pub fn set_compact_tokens(&mut self, tokens: Option<u32>) {
        self.compact_tokens = tokens;
    }

    pub fn set_fallbacks(
        &mut self,
        fallbacks: impl IntoIterator<Item = (P, crate::endpoint::Endpoint)>,
    ) {
        self.fallbacks = fallbacks.into_iter().collect();
    }

    pub fn apply_endpoint(&mut self, endpoint: &crate::endpoint::Endpoint) {
        self.set_context_window(endpoint.context_window);
        self.set_compact_tokens(endpoint.compact_tokens);
        self.set_effort(endpoint.effort_for(self.chosen_effort));
        self.set_price(endpoint.price);
        self.set_env(endpoint.env.clone());
    }

    pub(crate) fn set_seat(
        &mut self,
        role: workflow::Role,
        provider: P,
        model: impl Into<String>,
        effort: Option<crate::provider::Effort>,
        price: Option<crate::provider::Price>,
        context_window: Option<u32>,
    ) {
        self.seats
            .insert(role, (provider, model.into(), context_window));
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
        self.seats.get(&role).map(|(_, model, _)| model.as_str())
    }

    pub(crate) fn set_workflow(&mut self, name: Option<&str>) -> Result<()> {
        self.session.set_workflow(name).map_err(Error::from)
    }

    pub(crate) fn apply_workflow(
        &mut self,
        workflow: &workflow::Workflow,
        mut resolve: impl FnMut(
            workflow::Role,
            &workflow::Seat,
        ) -> Result<(P, crate::endpoint::Endpoint)>,
    ) -> Result<()> {
        let mut seats = BTreeMap::new();
        let mut tuning = BTreeMap::new();
        for (role, seat) in &workflow.seats {
            let (provider, endpoint) = resolve(*role, seat).map_err(|error| {
                Error::Config(format!("{}: {role} seat {seat}: {error}", workflow.name))
            })?;
            seats.insert(*role, (provider, endpoint.model, endpoint.context_window));
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
        self.check_file = std::fs::read(crate::verify::check_path(&self.workspace)).ok();
        self.verify = commands;
    }

    #[must_use]
    pub fn with_verify(mut self, commands: Vec<String>) -> Self {
        self.set_verify(Some(commands));
        self
    }

    pub fn with_budget(mut self, budget: Budget) -> Result<Self> {
        self.session.set_budget(
            budget.max_tokens,
            budget.max_secs.map(|seconds| seconds.saturating_mul(1_000)),
        )?;
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

    pub fn set_name(&mut self, name: &str) -> Result<()> {
        crate::session::set_name(self.session.path(), name)?;
        if self.mailbox.is_some() {
            self.join_mail();
        }
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
        self.session.work_progress().map_err(Error::from)
    }

    pub fn next_beat(&self, job: &crate::Job) -> Result<crate::Beat> {
        let progress = self.work_progress()?;
        if let Some(code) = progress.exit_code() {
            return Ok(crate::Beat::Done(code));
        }
        let (plan, steps) = match job {
            crate::Job::Goal { objective, gates } => {
                return Ok(crate::Beat::Run {
                    prompt: crate::goal::prompt(objective, gates, progress.rounds),
                    step: None,
                });
            }
            crate::Job::Steps { plan, steps } => (plan, steps),
        };
        let current = progress
            .current
            .ok_or_else(|| Error::Config("Steps Work has no active item".to_string()))?;
        let step = steps
            .get(current.saturating_sub(1))
            .ok_or_else(|| Error::Config("Steps Work is outside the selected plan".to_string()))?;
        Ok(crate::Beat::Run {
            prompt: crate::steps::prompt(step, steps.len(), plan),
            step: Some((current, steps.len())),
        })
    }
}

impl Loop<crate::provider::Client> {
    pub fn seat_workflow(
        &mut self,
        workflow: &workflow::Workflow,
        mut connect: impl FnMut(
            &workflow::Seat,
        ) -> Result<(crate::provider::Client, crate::endpoint::Endpoint)>,
    ) -> Result<()> {
        let mut missing = Vec::new();
        self.apply_workflow(workflow, |role, seat| {
            let (client, endpoint) = connect(seat)?;
            if endpoint.context_window.is_none() {
                missing.push((
                    role,
                    client.clone(),
                    endpoint.name.clone(),
                    endpoint.model.clone(),
                ));
            }
            Ok((client, endpoint))
        })?;
        for (role, client, name, model) in missing {
            self.watch_context(Some(role), client, &name, &model);
        }
        Ok(())
    }

    pub fn watch_driver(&mut self, client: crate::provider::Client, endpoint: &str, model: &str) {
        self.watch_context(None, client, endpoint, model);
    }

    pub(crate) fn watch_context(
        &mut self,
        role: Option<workflow::Role>,
        client: crate::provider::Client,
        endpoint: &str,
        model: &str,
    ) {
        let endpoint = endpoint.to_string();
        let model = model.to_string();
        self.watch_window(
            role,
            tokio::spawn(async move { client.context(&endpoint, &model).await }),
        );
    }
}
