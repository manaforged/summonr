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
            crate::prompt::mage_home()?,
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
        let child = parent.fork().map_err(super::session_err)?;
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

    pub fn with_session(
        provider: P,
        model: impl Into<String>,
        workspace: impl AsRef<Path>,
        system_prompt: impl Into<String>,
    ) -> Result<Self> {
        let workspace = workspace.as_ref();
        let model = model.into();
        let system_prompt = system_prompt.into();
        let session = open_driver(workspace, workspace, &model, &system_prompt)?;
        Self::with_parts(
            provider,
            model,
            workspace,
            system_prompt,
            session,
            Policy::builtin(),
            None,
        )
    }

    fn with_parts(
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
            return Err(Error::Config("MAGE_MODEL is empty".to_string()));
        }
        let system_prompt = system_prompt.into();
        let (max_tokens, max_live_millis) = session.budget_limits();
        Ok(Self {
            provider,
            model,
            seats: BTreeMap::new(),
            system_prompt,
            plan: false,
            tools: ToolRegistry::with_policy_and_search(workspace, policy, search)?,
            schema: None,
            history: Vec::new(),
            session,
            max_rounds: DEFAULT_MAX_ROUNDS,
            verify: None,
            budget: Budget {
                max_tokens,
                max_secs: max_live_millis
                    .map(|millis| millis.saturating_add(999).saturating_div(1_000)),
            },
            spent_tokens: 0,
            context_sample: None,
            context_window: None,
            workspace: workspace
                .canonicalize()
                .unwrap_or_else(|_| workspace.to_path_buf()),
            bus: None,
            mail: std::collections::VecDeque::new(),
            steer: std::collections::VecDeque::new(),
            steer_shared: None,
        })
    }

    pub fn steer(&mut self, input: impl Into<UserInput>) {
        if let Some(shared) = &self.steer_shared
            && let Ok(mut queue) = shared.lock()
        {
            queue.push_back(input.into());
            return;
        }
        self.steer.push_back(input.into());
    }

    pub fn steer_with(
        &mut self,
        source: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<UserInput>>>,
    ) {
        self.steer_shared = Some(source);
    }

    pub fn set_env(&mut self, env: impl IntoIterator<Item = (String, String)>) {
        self.tools.set_env(env);
    }

    pub fn with_sandbox(mut self, deny_network: bool) -> Result<Self> {
        self.tools.enable_sandbox(deny_network)?;
        Ok(self)
    }

    pub fn set_context_window(&mut self, window: Option<u32>) {
        self.context_window = window;
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
    pub fn with_plan(mut self) -> Self {
        self.set_plan(true);
        self
    }

    pub fn set_plan(&mut self, plan: bool) {
        if self.plan == plan {
            return;
        }
        self.plan = plan;
        self.tools.set_read_only(plan);
    }

    #[must_use]
    pub fn is_plan(&self) -> bool {
        self.plan
    }

    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    #[must_use]
    pub fn max_rounds(&self) -> usize {
        self.max_rounds
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
    pub fn spent_tokens(&self) -> u64 {
        self.spent_tokens
    }

    #[must_use]
    pub fn elapsed_secs(&self) -> u64 {
        self.session.replay().elapsed_millis / 1000
    }

    #[must_use]
    pub fn is_strict(&self) -> bool {
        self.tools.is_strict()
    }

    pub fn set_provider(&mut self, provider: P, model: impl Into<String>) {
        self.provider = provider;
        self.model = model.into();
        self.context_sample = None;
    }

    pub fn set_seat(&mut self, role: workflow::Role, provider: P, model: impl Into<String>) {
        self.seats.insert(role, (provider, model.into()));
    }

    pub fn clear_seat(&mut self, role: workflow::Role) {
        self.seats.remove(&role);
    }

    #[must_use]
    pub fn seat_model(&self, role: workflow::Role) -> Option<&str> {
        self.seats.get(&role).map(|(_, model)| model.as_str())
    }

    pub fn set_workflow(&mut self, name: Option<&str>) -> Result<()> {
        self.session.set_workflow(name).map_err(super::session_err)
    }

    #[must_use]
    pub fn workflow(&self) -> Option<String> {
        self.session.workflow().map(str::to_string)
    }

    pub fn set_verify(&mut self, commands: Option<Vec<String>>) {
        if self.verify == commands {
            return;
        }
        self.verify = commands;
    }

    pub fn set_max_rounds(&mut self, max: usize) {
        self.max_rounds = max.max(1);
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
    pub fn with_max_rounds(mut self, max_rounds: usize) -> Self {
        self.max_rounds = max_rounds.max(1);
        self
    }

    #[must_use]
    pub fn session_path(&self) -> &Path {
        self.session.path()
    }

    #[cfg(test)]
    pub(crate) fn replay_session(&self) -> crate::session::Replay {
        self.session.replay()
    }

    #[must_use]
    pub fn session_id(&self) -> String {
        self.session.id().to_string()
    }

    pub fn work_progress(&self) -> Result<crate::WorkProgress> {
        self.session.work_progress().map_err(super::session_err)
    }

    pub fn active_goal(&self) -> Result<Option<(String, crate::WorkProgress)>> {
        let Some(objective) = self.session.goal_objective().map_err(super::session_err)? else {
            return Ok(None);
        };
        let progress = self.work_progress()?;
        Ok((!progress.terminal()).then_some((objective, progress)))
    }

    #[must_use]
    pub fn goal_status(&self) -> Option<String> {
        let (objective, progress) = self.active_goal().ok().flatten()?;
        let mut text = format!(
            "goal active — {}\nround {}/{} · {} tokens · {}",
            truncate_line(&objective, 80),
            progress.rounds,
            progress.max_turns,
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
    pub fn restore_replay(mut self, replay: crate::session::Replay) -> Self {
        self.history = replay.history;
        self.spent_tokens = replay.spent_tokens;
        self
    }

    pub fn fork_live(&mut self) -> Result<String> {
        let path = self.session.fork().map_err(super::session_err)?;
        let child = resume_driver(&self.workspace, &path)?;
        let replay = child.replay();
        let id = child.id().to_string();
        self.adopt(child, replay);
        Ok(id)
    }

    pub(crate) fn adopt(&mut self, session: SessionDriver, replay: crate::session::Replay) {
        self.session = session;
        self.history = replay.history;
        self.spent_tokens = replay.spent_tokens;
        self.bus = None;
        self.join_bus();
        self.schema = None;
    }

    #[must_use]
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub(crate) async fn judge(&mut self, system: &str, user: &str) -> Result<(String, Usage)> {
        let request = CompletionRequest {
            model: self.seat(workflow::Role::Judge).1.to_string(),
            messages: vec![
                Message::system(system.to_string()),
                Message::user(user.to_string()),
            ],
            tools: Vec::new(),
            max_output_tokens: JUDGE_OUTPUT_TOKENS,
            emit_deltas: false,
        };
        let durable = request
            .durable_bytes()
            .map_err(|error| Error::Config(format!("cannot encode provider request: {error}")))?;
        let attempt = self
            .session
            .start_judge(&request.model, super::exposure(&request, &durable))
            .map_err(super::session_err)?;
        let started = Instant::now();
        match self.seat(workflow::Role::Judge).0.complete(&request).await {
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
                Ok((text, usage))
            }
            Err(error) => {
                self.session
                    .fail_provider(attempt, &error.to_string(), super::elapsed_millis(started))
                    .map_err(super::session_err)?;
                self.spent_tokens = self.session.replay().spent_tokens;
                Err(error)
            }
        }
    }

    #[must_use]
    pub fn tool_names(&self) -> Vec<String> {
        if let Some(schema) = &self.schema {
            return schema
                .iter()
                .map(|tool| tool.function.name.clone())
                .collect();
        }
        self.tool_definitions()
            .into_iter()
            .map(|tool| tool.function.name)
            .collect()
    }

    pub fn clear_history(&mut self) -> Result<()> {
        self.session.clear().map_err(super::session_err)?;
        self.history.clear();
        Ok(())
    }

    pub fn abort_note(&mut self, reason: &str) -> Result<()> {
        self.session.abort(reason).map_err(super::session_err)
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

    pub fn accept_plan(&mut self) -> Result<()> {
        self.session.accept_plan().map_err(super::session_err)
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
