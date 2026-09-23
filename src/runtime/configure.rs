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

    pub fn set_images(&mut self, model_images: bool, vision: Option<crate::endpoint::Endpoint>) {
        self.tools.set_images(model_images, vision);
    }

    pub fn with_sandbox(mut self, deny_network: bool) -> Result<Self> {
        self.tools.enable_sandbox(deny_network)?;
        Ok(self)
    }

    pub(crate) fn set_network(&mut self, allow: bool) -> Result<()> {
        if self.tools.has_sandbox() {
            self.tools.enable_sandbox(!allow)?;
        }
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
        drop(self.set_mode(crate::RunMode::Plan));
        self
    }

    pub(crate) fn set_mode(&mut self, mode: crate::RunMode) -> Result<()> {
        let plan = mode == crate::RunMode::Plan
            || mode == crate::RunMode::Work && !self.session.has_selected_plan();
        let mode = if plan { crate::RunMode::Plan } else { mode };
        if std::mem::replace(&mut self.mode, mode) == mode {
            return Ok(());
        }
        let sandbox = (plan && !self.tools.has_sandbox()).then(|| self.tools.enable_sandbox(false));
        self.tools.set_read_only(plan);
        self.schema = None;
        if let Some(result) = sandbox {
            result?;
        }
        Ok(())
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
        self.session.elapsed_millis() / 1000
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
        let round = progress.rounds;
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
    pub(crate) fn restore_replay(mut self, replay: crate::session::Replay) -> Self {
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
        let messages = [
            Message::system(system.to_string()),
            Message::user(user.to_string()),
        ];
        let request = self.request(workflow::Role::Judge, &messages, &[], false);
        match self.complete_once(workflow::Role::Judge, &request).await? {
            Ok(message) => {
                let usage = message.usage.clone();
                let text = message.content.unwrap_or_default();
                if let Some(reason) = self.round_abort() {
                    return Err(Error::Config(reason));
                }
                Ok((text, usage))
            }
            Err(wait_error) => Err(wait_error.error()),
        }
    }

    pub(crate) fn clear_history(&mut self) -> Result<()> {
        self.session.clear().map_err(super::session_err)?;
        self.tools.forget_reads();
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

    pub fn select_goal(&mut self, objective: &str, commands: &[String]) -> Result<()> {
        self.session
            .prepare_external_work(objective, WorkPolicy::Goal {}, commands)
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
    pub fn goal_at(path: &Path, workspace: &Path, objective: &str) -> bool {
        work_at(path, workspace, WorkPolicy::Goal {}, objective)
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
