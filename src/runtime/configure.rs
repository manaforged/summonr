use super::*;
use crate::durable::driver::work_at;
use crate::durable::log;
use crate::durable::work::Policy as WorkPolicy;
use crate::workflow;

impl<P: Provider> Loop<P> {
    pub fn new(
        provider: P,
        model: impl Into<String>,
        workspace: impl AsRef<Path>,
        system_prompt: impl Into<String>,
    ) -> Result<Self> {
        let workspace = workspace.as_ref();
        let model = model.into();
        let system_prompt = system_prompt.into();
        let session = open_driver(
            crate::prompt::summonr_home()?,
            workspace,
            &model,
            &system_prompt,
        )?;
        let mut loop_ = Self::with_parts(
            provider,
            model,
            workspace,
            system_prompt,
            session,
            Policy::discover(workspace)?,
            Some(crate::web::Search::discover()?),
        )?;
        loop_.join_bus();
        Ok(loop_)
    }

    pub fn resume_path(
        provider: P,
        model: impl Into<String>,
        workspace: impl AsRef<Path>,
        system_prompt: impl Into<String>,
        path: &Path,
    ) -> Result<Self> {
        let session = resume_driver(workspace.as_ref(), path)?;
        let replay = session.replay();
        let workspace = session.workspace().to_path_buf();
        let mut loop_ = Self::open(provider, model, workspace, system_prompt, session)?;
        loop_ = loop_.restore_replay(replay);
        Ok(loop_)
    }

    pub fn fork_path(
        provider: P,
        model: impl Into<String>,
        workspace: impl AsRef<Path>,
        system_prompt: impl Into<String>,
        source: &Path,
    ) -> Result<Self> {
        let parent = resume_driver(workspace.as_ref(), source)?;
        let child = parent.fork(None).map_err(super::session_err)?;
        Self::resume_path(provider, model, workspace, system_prompt, &child)
    }

    pub(crate) fn open(
        provider: P,
        model: impl Into<String>,
        workspace: impl AsRef<Path>,
        system_prompt: impl Into<String>,
        session: SessionDriver,
    ) -> Result<Self> {
        let workspace = workspace.as_ref();
        let mut loop_ = Self::with_parts(
            provider,
            model,
            workspace,
            system_prompt,
            session,
            Policy::discover(workspace)?,
            Some(crate::web::Search::discover()?),
        )?;
        loop_.join_bus();
        Ok(loop_)
    }

    pub(super) fn with_parts(
        provider: P,
        model: impl Into<String>,
        workspace: impl AsRef<Path>,
        system_prompt: impl Into<String>,
        session: SessionDriver,
        policy: Policy,
        search: Option<crate::web::Search>,
    ) -> Result<Self> {
        let workspace = workspace.as_ref();
        let model = model.into();
        if model.trim().is_empty() {
            return Err(Error::Config("SUMMONR_MODEL is empty".to_string()));
        }
        let system_prompt = system_prompt.into();
        let (max_tokens, max_live_millis) = session.budget_limits();
        let depth = session.depth();
        let tools = ToolRegistry::with_policy_and_search(workspace, policy, search)?;
        tools.set_session(&session.id().to_string());
        let herdr =
            crate::herdr::Reporter::discover(&session.id().to_string(), workspace, &model, depth);
        Ok(Self {
            provider,
            model,
            seats: BTreeMap::new(),
            tuning: BTreeMap::new(),
            price: None,
            system_prompt,
            mode: crate::RunMode::Auto,
            plan_first: false,
            tools,
            schema: None,
            history: Vec::new(),
            herdr,
            session,
            verify: None,
            budget: Budget {
                max_tokens,
                max_secs: max_live_millis
                    .map(|millis| millis.saturating_add(999).saturating_div(1_000)),
            },
            spent_tokens: 0,
            context_sample: None,
            context_window: None,
            probe: None,
            effort: None,
            turn_started: 0,
            accepted: false,
            summary_failed: false,
            workspace: workspace
                .canonicalize()
                .unwrap_or_else(|_| workspace.to_path_buf()),
            bus: None,
            mcp: None,
            guard: super::guard::Guard::default(),
            checkpoints: None,
            fallbacks: VecDeque::new(),
            compact_tokens: None,
            hooks: crate::config::load()
                .map(|file| file.hooks)
                .unwrap_or_default(),
            mcp_start: None,
            mail: std::collections::VecDeque::new(),
            steer: std::collections::VecDeque::new(),
            steer_shared: None,
            depth,
            tasks: true,
            manual: false,
            asked: Vec::new(),
            blocks: (0, 0),
            spawned: BTreeMap::new(),
            mailbox: tokio::sync::mpsc::unbounded_channel(),
            catalog_frozen: false,
        })
    }

    pub(crate) fn steer_with(
        &mut self,
        source: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<UserInput>>>,
    ) {
        self.steer_shared = Some(source);
    }

    pub fn set_modules(&mut self, switches: &crate::modules::Switches) {
        if self.catalog_frozen {
            return;
        }
        self.tools.set_modules(
            switches.enabled("search"),
            switches.enabled("lsp"),
            switches.enabled("jobs"),
            switches.enabled("skills"),
        );
        self.tasks = switches.enabled("task");
        if !switches.enabled("bus") {
            self.bus = None;
        }
        self.switch_mcp(switches.enabled("mcp"));
        self.schema = None;
    }

    #[must_use]
    pub(crate) fn catalog_frozen(&self) -> bool {
        self.catalog_frozen
    }

    pub(crate) fn freeze_catalog(&mut self) {
        self.catalog_frozen = true;
    }

    pub fn set_env(&mut self, env: impl IntoIterator<Item = (String, String)>) {
        self.tools.set_env(env);
    }

    pub fn with_sandbox(mut self, deny_network: bool) -> Result<Self> {
        self.tools.enable_sandbox(deny_network)?;
        Ok(self)
    }

    pub(crate) fn set_network(&mut self, allow: bool) -> Result<()> {
        if self.tools.has_sandbox() {
            self.tools.enable_sandbox(!allow)?;
        }
        self.schema = None;
        Ok(())
    }

    pub(crate) fn allow_network_once(&self) {
        self.tools.allow_network_once();
    }

    pub(crate) fn set_manual(&mut self, on: bool) {
        self.manual = on;
    }

    #[must_use]
    pub(crate) fn manual(&self) -> bool {
        self.manual
    }

    pub fn allow_path(&mut self, path: &str) -> Result<()> {
        self.tools.allow_path(crate::policy::expand_path(path))
    }

    #[must_use]
    pub(crate) fn write_roots(&self) -> Vec<String> {
        self.tools.write_roots()
    }

    #[must_use]
    pub(crate) fn has_sandbox(&self) -> bool {
        self.tools.has_sandbox()
    }

    #[must_use]
    pub(crate) fn denies_network(&self) -> bool {
        self.tools.denies_network()
    }

    pub fn set_context_window(&mut self, window: Option<u32>) {
        self.context_window = window;
    }

    pub fn set_context_probe(&mut self, probe: tokio::task::JoinHandle<Option<u32>>) {
        self.probe = Some(probe);
    }

    pub(crate) async fn settle_probe(&mut self) {
        if self
            .probe
            .as_ref()
            .is_some_and(tokio::task::JoinHandle::is_finished)
            && let Some(probe) = self.probe.take()
            && let Ok(Some(window)) = probe.await
        {
            self.context_window = Some(self.context_window.map_or(window, |have| have.min(window)));
        }
    }

    pub fn set_effort(&mut self, effort: Option<crate::provider::Effort>) {
        self.effort = effort;
    }

    #[must_use]
    pub fn effort(&self) -> Option<crate::provider::Effort> {
        self.effort
    }

    #[must_use]
    pub fn context_window(&self) -> Option<u32> {
        self.context_window
    }

    #[must_use]
    pub fn with_strict(mut self) -> Self {
        self.tools.set_strict();
        self
    }

    #[must_use]
    pub fn with_unrestricted_commands(mut self) -> Self {
        self.tools.set_unrestricted();
        self
    }

    #[must_use]
    pub fn with_plan(mut self) -> Self {
        self.set_mode(crate::RunMode::Plan);
        self
    }

    /// Write a model plan before an ordinary turn executes.
    pub(crate) fn set_mode(&mut self, mode: crate::RunMode) {
        let mode = if mode == crate::RunMode::Work && !self.session.has_selected_plan() {
            crate::RunMode::Plan
        } else {
            mode
        };
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        self.tools.set_read_only(mode == crate::RunMode::Plan);
        self.schema = None;
    }

    #[must_use]
    pub(crate) fn is_plan(&self) -> bool {
        self.mode == crate::RunMode::Plan
    }

    #[must_use]
    pub(crate) fn is_work(&self) -> bool {
        self.mode == crate::RunMode::Work
    }

    #[must_use]
    pub(crate) fn is_goal(&self) -> bool {
        self.mode == crate::RunMode::Goal
    }

    #[must_use]
    pub fn mode(&self) -> crate::RunMode {
        self.mode
    }

    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    #[must_use]
    pub fn verify_commands(&self) -> &[String] {
        self.verify.as_deref().unwrap_or(&[])
    }

    #[must_use]
    pub fn budget(&self) -> &Budget {
        &self.budget
    }

    #[must_use]
    pub(crate) fn spent_tokens(&self) -> u64 {
        self.spent_tokens
    }

    #[must_use]
    pub(crate) fn elapsed_secs(&self) -> u64 {
        self.session.replay().elapsed_millis / 1000
    }

    #[must_use]
    pub(crate) fn is_strict(&self) -> bool {
        self.tools.is_strict()
    }

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
        if self.verify == commands {
            return;
        }
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
        self.history.len()
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

    pub(crate) fn active_goal(&self) -> Result<Option<(String, crate::WorkProgress)>> {
        let Some(objective) = self.session.goal_objective().map_err(super::session_err)? else {
            return Ok(None);
        };
        let progress = self.work_progress()?;
        Ok((!progress.terminal()).then_some((objective, progress)))
    }

    #[must_use]
    pub(crate) fn goal_status(&self) -> Option<String> {
        let (objective, progress) = self.active_goal().ok().flatten()?;
        let round = if progress.max_turns == 0 {
            progress.rounds.to_string()
        } else {
            format!("{}/{}", progress.rounds, progress.max_turns)
        };
        let mut text = format!(
            "goal active — {}\nround {round} · {} tokens · {}",
            truncate_line(&objective, 80),
            self.spent_tokens(),
            progress.phase,
        );
        let gates = self.verify_commands();
        if !gates.is_empty() {
            text.push_str("\ngates: ");
            text.push_str(&gates.join(" then "));
        }
        if let Some(reason) = &progress.reason {
            text.push_str("\nlast: ");
            text.push_str(&truncate_line(reason, 160));
        }
        Some(text)
    }

    pub fn abandon_work(&mut self, reason: &str) -> Result<()> {
        self.session
            .abandon_work(reason)
            .map_err(super::session_err)
    }

    #[must_use]
    pub fn restore(mut self, history: Vec<Message>) -> Self {
        self.history = history;
        self
    }

    #[must_use]
    pub(crate) fn restore_replay(mut self, replay: crate::session::Replay) -> Self {
        self.history = replay.history;
        self.spent_tokens = replay.spent_tokens;
        self
    }

    pub(crate) fn fork_live(&mut self) -> Result<String> {
        self.fork_at(None)
    }

    pub(crate) fn fork_at(&mut self, turn: Option<usize>) -> Result<String> {
        if !self.spawned.is_empty() {
            return Err(Error::Config(
                "Collect or stop background agents before branching".to_string(),
            ));
        }
        let path = self.session.fork(turn).map_err(super::session_err)?;
        let child = resume_driver(&self.workspace, &path)?;
        let replay = child.replay();
        let id = child.id().to_string();
        self.adopt(child, replay);
        Ok(id)
    }

    pub(crate) fn adopt(&mut self, session: SessionDriver, replay: crate::session::Replay) {
        self.forget_children();
        self.session = session;
        self.tools.set_session(&self.session.id().to_string());
        self.history = replay.history;
        self.spent_tokens = replay.spent_tokens;
        self.bus = None;
        self.join_bus();
        self.schema = None;
        self.herdr.retarget(&self.session.id().to_string());
    }

    #[must_use]
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub(crate) async fn judge(&mut self, system: &str, user: &str) -> Result<(String, Usage)> {
        let request = CompletionRequest {
            session: self.session_id(),
            model: self.seat(workflow::Role::Judge).1.to_string(),
            messages: vec![
                Message::system(system.to_string()),
                Message::user(user.to_string()),
            ],
            tools: Vec::new(),
            max_output_tokens: None,
            emit_deltas: false,
            effort: self.effort_for(workflow::Role::Judge),
        };
        let durable = request
            .durable_bytes()
            .map_err(|error| Error::Config(format!("cannot encode provider request: {error}")))?;
        if let Some(reason) = self.round_abort() {
            return Err(Error::Config(reason));
        }
        let attempt = self
            .session
            .start_judge(&request.model, super::exposure(&request, &durable))
            .map_err(super::session_err)?;
        let started = Instant::now();
        let completion = self.seat(workflow::Role::Judge).0.complete(&request);
        match Self::wait(completion, self.remaining_time()).await {
            Ok(message) => {
                let usage = message.usage.clone();
                let text = message.content.unwrap_or_default();
                let meter = super::meter(&usage, super::elapsed_millis(started));
                self.session
                    .finish_provider(attempt, &text, "", meter)
                    .map_err(super::session_err)?;
                self.spent_tokens = self.spent_tokens.saturating_add(u64::from(
                    usage.prompt_tokens.saturating_add(usage.completion_tokens),
                ));
                if let Some(reason) = self.round_abort() {
                    return Err(Error::Config(reason));
                }
                Ok((text, usage))
            }
            Err(wait_error) => {
                let millis = wait_error.millis(started);
                let error = wait_error.error();
                self.session
                    .fail_provider(attempt, &error.to_string(), millis, super::billable(&error))
                    .map_err(super::session_err)?;
                self.spent_tokens = self.session.replay().spent_tokens;
                Err(error)
            }
        }
    }

    pub(crate) fn clear_history(&mut self) -> Result<()> {
        self.session.clear().map_err(super::session_err)?;
        self.history.clear();
        Ok(())
    }

    pub(crate) fn abort_note(&mut self, reason: &str) -> Result<()> {
        self.abort_with(reason, |_| {})
    }

    pub(crate) fn abort_with(&mut self, reason: &str, mut emit: impl FnMut(&Event)) -> Result<()> {
        self.forget_children();
        self.session
            .abort(reason, None)
            .map_err(super::session_err)?;
        self.hydrate();
        self.sync_usage(&mut emit);
        Ok(())
    }

    pub(crate) fn unreported_tokens(&self) -> u64 {
        self.session.usage().1
    }

    fn forget_children(&mut self) {
        for spawned in self.spawned.values() {
            spawned.abort();
        }
        self.spawned.clear();
    }

    pub fn select_steps(&mut self, markdown: &str, commands: &[String]) -> Result<()> {
        self.session
            .prepare_external_work(markdown, WorkPolicy::Steps, commands)
            .map_err(super::session_err)
    }

    pub fn select_goal(
        &mut self,
        objective: &str,
        commands: &[String],
        max_turns: usize,
    ) -> Result<()> {
        let max_turns = u32::try_from(max_turns)
            .map_err(|_error| Error::Config("goal turn limit is too large".to_string()))?;
        self.session
            .prepare_external_work(objective, WorkPolicy::Goal { max_turns }, commands)
            .map_err(super::session_err)
    }

    pub(crate) fn accept_plan(&mut self) -> Result<()> {
        self.session.accept_plan().map_err(super::session_err)
    }

    pub(crate) fn pending_plan(&self) -> bool {
        self.session.pending_plan().is_ok()
    }

    pub(crate) fn reject_plan(&mut self) -> Result<()> {
        self.session.reject_plan().map_err(super::session_err)
    }

    #[must_use]
    pub fn touched_at(path: &Path) -> Vec<String> {
        crate::durable::driver::touched_at(path)
    }

    #[must_use]
    pub fn job_at(path: &Path, workspace: &Path) -> Option<(crate::WorkKind, String)> {
        let (policy, objective) = crate::durable::driver::active_at(path, workspace)?;
        let kind = match policy {
            WorkPolicy::Single => crate::WorkKind::Single,
            WorkPolicy::Steps => crate::WorkKind::Steps,
            WorkPolicy::Goal { .. } => crate::WorkKind::Goal,
        };
        Some((kind, objective))
    }

    #[must_use]
    pub fn steps_at(path: &Path, workspace: &Path, markdown: &str) -> bool {
        work_at(path, workspace, WorkPolicy::Steps, markdown)
    }

    #[must_use]
    pub fn goal_at(path: &Path, workspace: &Path, objective: &str, max_turns: usize) -> bool {
        let Ok(max_turns) = u32::try_from(max_turns) else {
            return false;
        };
        work_at(path, workspace, WorkPolicy::Goal { max_turns }, objective)
    }
}

fn resume_driver(workspace: &Path, path: &Path) -> Result<SessionDriver> {
    let (Some(home), Some(session)) = (log::home_of(path), log::parse_path(path)) else {
        return Err(Error::Config(format!(
            "{} is not a session log",
            path.display()
        )));
    };
    SessionDriver::resume(home, session, workspace).map_err(super::session_err)
}

fn open_driver(
    home: impl AsRef<Path>,
    workspace: &Path,
    model: &str,
    system: &str,
) -> Result<SessionDriver> {
    SessionDriver::open(home.as_ref(), workspace, model, system).map_err(super::session_err)
}

fn truncate_line(text: &str, max: usize) -> String {
    let mut cut = text.find('\n').unwrap_or(text.len()).min(max);
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let line = &text[..cut];
    if line.len() < text.len() {
        format!("{line}…")
    } else {
        line.to_string()
    }
}
