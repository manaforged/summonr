use super::*;

impl<P: Provider> Loop<P> {
    /// Discover policy files and start a new session under `$MAGE_HOME`.
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

    /// Resume a format-2 journal, or import a v1 `sessions/` log once.
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

    /// Fork a format-2 journal into a new child session.
    pub fn fork_path(
        provider: P,
        model: impl Into<String>,
        workspace: impl AsRef<Path>,
        system_prompt: impl Into<String>,
        source: &Path,
    ) -> Result<Self> {
        let mut parent = resume_driver(workspace.as_ref(), source)?;
        let child = crate::durable::id::SessionId::from_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                ^ u128::from(std::process::id()),
        );
        let intent = parent
            .fork(child, crate::durable::fact::ForkAdoption::AdoptChild)
            .map_err(super::session_err)?;
        let child_path = crate::durable::journal::session_path(&crate::prompt::mage_home()?, child);
        parent
            .publish_fork(intent, &child_path)
            .map_err(super::session_err)?;
        Self::resume_path(provider, model, workspace, system_prompt, &child_path)
    }

    /// Create a loop with discovered policy and a caller-owned session.
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

    /// Create a hermetic loop. The journal lives under `workspace/sessions-v2`.
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
        Ok(Self {
            provider,
            model,
            system_prompt,
            plan: false,
            tools: ToolRegistry::with_policy_and_search(workspace, policy, search)?,
            schema: None,
            task_enabled: true,
            task: None,
            history: Vec::new(),
            session,
            max_rounds: DEFAULT_MAX_ROUNDS,
            verify: None,
            budget: Budget::none(),
            spent_tokens: 0,
            context_sample: None,
            started: Instant::now(),
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

    /// Steer the running turn: `text` lands as a user message before
    /// the next provider round, ahead of any bus mail.
    pub fn steer(&mut self, input: impl Into<UserInput>) {
        if let Some(shared) = &self.steer_shared
            && let Ok(mut queue) = shared.lock()
        {
            queue.push_back(input.into());
            return;
        }
        self.steer.push_back(input.into());
    }

    /// Share the worker's steering queue so steering lands mid-turn.
    pub fn steer_with(
        &mut self,
        source: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<UserInput>>>,
    ) {
        self.steer_shared = Some(source);
    }

    /// The endpoint's context window in tokens (drives compaction).
    /// Environment added to every `run_command` (the seat's clean toolchain).
    pub fn set_env(&mut self, env: impl IntoIterator<Item = (String, String)>) {
        self.tools.set_env(env);
    }

    /// Run model-executed commands inside the host OS filesystem sandbox.
    /// `deny_network` also denies sockets (`--no-net`), closing the
    /// read-anything-plus-post-anywhere path.
    pub fn with_sandbox(mut self, deny_network: bool) -> Result<Self> {
        self.tools.enable_sandbox(deny_network)?;
        Ok(self)
    }

    pub fn set_context_window(&mut self, window: Option<u32>) {
        self.context_window = window;
    }

    /// The endpoint's context window, if known.
    #[must_use]
    pub fn context_window(&self) -> Option<u32> {
        self.context_window
    }

    /// Deny every command not on the policy allow list.
    #[must_use]
    pub fn with_strict(mut self) -> Self {
        self.tools.set_strict();
        self
    }

    /// Plan mode: read-only tools, enforced at the policy layer.
    #[must_use]
    pub fn with_plan(mut self) -> Self {
        self.set_plan(true);
        self
    }

    /// Use an external step runner as the only task-completion authority.
    #[must_use]
    pub fn with_external_steps(mut self) -> Self {
        self.task_enabled = false;
        self.task = None;
        self
    }

    /// Runtime mode switch (interface mode key, `/plan` in the REPL).
    /// The provider schema stays fixed; policy enforces the new mode.
    pub fn set_plan(&mut self, plan: bool) {
        if self.plan == plan {
            return;
        }
        self.plan = plan;
        self.tools.set_read_only(plan);
    }

    /// True while plan mode is active.
    #[must_use]
    pub fn is_plan(&self) -> bool {
        self.plan
    }

    /// Current model name.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Round ceiling for one turn.
    #[must_use]
    pub fn max_rounds(&self) -> usize {
        self.max_rounds
    }

    /// The armed verify commands, when the gate is on.
    #[must_use]
    pub fn verify_commands(&self) -> &[String] {
        self.verify.as_deref().unwrap_or(&[])
    }

    /// Session budget ceilings.
    #[must_use]
    pub fn budget(&self) -> &Budget {
        &self.budget
    }

    /// Tokens spent across all turns in this session.
    #[must_use]
    pub fn spent_tokens(&self) -> u64 {
        self.spent_tokens
    }

    /// Wall-clock seconds charged to this session.
    #[must_use]
    pub fn elapsed_secs(&self) -> u64 {
        self.started.elapsed().as_secs()
    }

    /// True when the strict command allowlist is active.
    #[must_use]
    pub fn is_strict(&self) -> bool {
        self.tools.is_strict()
    }

    /// Swap the provider and model live (endpoint switch). History stays.
    pub fn set_provider(&mut self, provider: P, model: impl Into<String>) {
        self.provider = provider;
        self.model = model.into();
        self.context_sample = None;
    }

    /// Arm or disarm the verify gate.
    pub fn set_verify(&mut self, commands: Option<Vec<String>>) {
        if self.verify == commands {
            return;
        }
        self.verify = commands;
    }

    /// Change the per-turn round ceiling.
    pub fn set_max_rounds(&mut self, max: usize) {
        self.max_rounds = max.max(1);
    }

    /// Run these checks after any mutating turn and report the verdict.
    #[must_use]
    pub fn with_verify(mut self, commands: Vec<String>) -> Self {
        self.set_verify(Some(commands));
        self
    }

    /// Set session budget ceilings.
    #[must_use]
    pub fn with_budget(mut self, budget: Budget) -> Self {
        self.budget = budget;
        self
    }

    /// Messages stored after the last committed round.
    #[must_use]
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// Change the maximum provider rounds for one turn.
    #[must_use]
    pub fn with_max_rounds(mut self, max_rounds: usize) -> Self {
        self.max_rounds = max_rounds.max(1);
        self
    }

    /// Path of the format-2 session journal.
    #[must_use]
    pub fn session_path(&self) -> &Path {
        self.session.path()
    }

    #[cfg(test)]
    pub(crate) fn replay_session(&self) -> crate::session::Replay {
        self.session.replay()
    }

    /// Session id (filename stem).
    #[must_use]
    pub fn session_id(&self) -> String {
        self.session.id().to_string()
    }

    /// Replace in-RAM history, used when resuming a log.
    #[must_use]
    pub fn restore(mut self, history: Vec<Message>) -> Self {
        self.history = history;
        self
    }

    /// Restore all session-owned runtime state reconstructed from a log.
    #[must_use]
    pub fn restore_replay(mut self, replay: crate::session::Replay) -> Self {
        self.history = replay.history;
        self.task = replay.task;
        self.spent_tokens = replay.spent_tokens;
        self
    }

    /// Restore cumulative accounting after a process restart.
    #[must_use]
    pub fn restore_progress(mut self, spent_tokens: u64, elapsed_secs: u64) -> Self {
        self.spent_tokens = spent_tokens;
        self.started = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(elapsed_secs))
            .unwrap_or_else(std::time::Instant::now);
        self
    }

    /// Fork this session into a child journal and continue on the child.
    pub fn fork_live(&mut self) -> Result<String> {
        let child = crate::durable::id::SessionId::from_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
                ^ u128::from(std::process::id()),
        );
        let intent = self
            .session
            .fork(child, crate::durable::fact::ForkAdoption::AdoptChild)
            .map_err(super::session_err)?;
        let home = self
            .session
            .path()
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| Error::Config("session path has no home".to_string()))?
            .to_path_buf();
        let child_path = crate::durable::journal::session_path(&home, child);
        self.session
            .publish_fork(intent, &child_path)
            .map_err(super::session_err)?;
        let child_driver = crate::durable::product::resume(&home, child, &self.workspace)
            .map_err(super::session_err)?;
        let replay = child_driver.replay();
        self.adopt(child_driver, replay);
        Ok(child.to_string())
    }

    /// Swap the live session for a fork: new log, replayed history, and
    /// a fresh bus identity. The old session file stays untouched.
    pub(crate) fn adopt(&mut self, session: SessionDriver, replay: crate::session::Replay) {
        self.session = session;
        self.history = replay.history;
        self.task = replay.task;
        self.bus = None;
        self.join_bus();
        self.schema = None;
    }

    #[must_use]
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    /// Run a shell command through the same policy as the `run_command` tool.
    pub async fn shell(&mut self, command: &str) -> Result<String> {
        self.tools
            .execute(&ToolCall {
                id: "repl".to_string(),
                kind: ToolKind::Function,
                function: ToolFunction {
                    name: "run_command".to_string(),
                    arguments: serde_json::json!({
                        "command": command,
                        "timeout_secs": 600,
                        "fresh": true
                    })
                    .to_string(),
                },
            })
            .await
    }

    /// One-shot auxiliary completion outside the session (the goal judge).
    /// No tools, no history, no session log.
    pub async fn aux(&mut self, system: &str, user: &str) -> Result<String> {
        let request = CompletionRequest {
            model: self.model.clone(),
            messages: vec![
                Message::system(system.to_string()),
                Message::user(user.to_string()),
            ],
            tools: Vec::new(),
            max_output_tokens: AUX_OUTPUT_TOKENS,
        };
        let message = self.provider.complete(&request).await?;
        Ok(message.content.unwrap_or_default())
    }

    /// Names of the tools currently advertised to the provider.
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

    /// Current model-owned task checklist, when one exists.
    #[must_use]
    pub fn task_state(&self) -> Option<&crate::task::TaskState> {
        self.task.as_ref()
    }

    /// Remove the current conversation while keeping the provider and tools.
    pub fn clear_history(&mut self) -> Result<()> {
        self.session.clear().map_err(super::session_err)?;
        self.history.clear();
        self.task = None;
        Ok(())
    }

    /// Record that the turn aborted (user cancel, shutdown).
    pub fn abort_note(&mut self, reason: &str) -> Result<()> {
        if self.session.state().operation.is_none() {
            return Ok(());
        }
        let abort = if reason.contains("shutdown") {
            crate::durable::context::AbortReason::Shutdown
        } else {
            crate::durable::context::AbortReason::User
        };
        self.session.abort(abort).map_err(super::session_err)?;
        Ok(())
    }

    /// Publish a Plan revision for the current Work.
    pub fn select_plan(&mut self, markdown: &str) -> Result<()> {
        if self.session.state().works.is_empty() {
            self.session
                .set_work(markdown)
                .map_err(super::session_err)?;
        }
        self.session
            .select_plan(markdown)
            .map_err(super::session_err)?;
        Ok(())
    }

    /// Journal step or goal progress on the active Work.
    pub fn persist_progress(&mut self, job: &crate::steps::JobState) -> Result<()> {
        let snapshot =
            serde_json::to_string(job).map_err(|error| Error::Config(error.to_string()))?;
        if self.session.state().works.is_empty() {
            let objective = job.goal.clone().unwrap_or_else(|| job.plan.clone());
            self.session
                .set_work(&objective)
                .map_err(super::session_err)?;
        }
        self.session.progress(&snapshot).map_err(super::session_err)
    }

    /// Restore step or goal progress from the journaled Work projection.
    #[must_use]
    pub fn restored_progress(&self) -> Option<crate::steps::JobState> {
        let snapshot = self.session.progress_text()?;
        if !snapshot.trim_start().starts_with('{') {
            return None;
        }
        serde_json::from_str(snapshot).ok()
    }

    /// Read a job snapshot from a format-2 journal without taking the writer.
    #[must_use]
    pub fn job_at(path: &Path) -> Option<crate::steps::JobState> {
        crate::durable::product::job_progress(path)
    }
}

fn resume_driver(workspace: &Path, path: &Path) -> Result<SessionDriver> {
    let home = path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| Error::Config("session path has no home".to_string()))?;
    if let Some(session) = crate::durable::journal::parse_path(path) {
        return crate::durable::product::resume(home, session, workspace)
            .map_err(super::session_err);
    }
    if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl")
        && path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            == Some("sessions")
    {
        return crate::durable::product::import(home, path, workspace).map_err(super::session_err);
    }
    Err(Error::Config(
        "session path is neither format-2 nor a v1 sessions log".to_string(),
    ))
}

fn open_driver(
    home: impl AsRef<Path>,
    workspace: &Path,
    model: &str,
    system: &str,
) -> Result<SessionDriver> {
    crate::durable::product::open(&crate::durable::product::Open {
        home: home.as_ref().to_path_buf(),
        workspace: workspace.to_path_buf(),
        model: model.to_string(),
        system: system.to_string(),
        endpoint: "https://example.test/v1".to_string(),
        adapter: "openai-compatible".to_string(),
    })
    .map_err(super::session_err)
}
