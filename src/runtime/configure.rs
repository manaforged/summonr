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

    /// Resume a format-2 journal.
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
        let (max_tokens, max_live_millis) = session.budget_limits();
        Ok(Self {
            provider,
            model,
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

    /// Current harness-owned Work progress.
    pub fn work_progress(&self) -> Result<crate::WorkProgress> {
        self.session.work_progress().map_err(super::session_err)
    }

    pub fn abandon_work(&mut self, reason: &str) -> Result<()> {
        self.session
            .abandon_work(reason)
            .map_err(super::session_err)
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
        self.spent_tokens = replay.spent_tokens;
        self.started = restored_start(replay.elapsed_millis);
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
        self.spent_tokens = replay.spent_tokens;
        self.started = restored_start(replay.elapsed_millis);
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

    /// Run the Goal judge as a durable, metered provider effect.
    pub(crate) async fn judge(&mut self, system: &str, user: &str) -> Result<(String, Usage)> {
        let request = CompletionRequest {
            model: self.model.clone(),
            messages: vec![
                Message::system(system.to_string()),
                Message::user(user.to_string()),
            ],
            tools: Vec::new(),
            max_output_tokens: JUDGE_OUTPUT_TOKENS,
            emit_deltas: false,
        };
        let attempt = self
            .session
            .start_judge(request.prefix_hash().as_bytes())
            .map_err(super::session_err)?;
        let started = Instant::now();
        match self.provider.complete(&request).await {
            Ok(message) => {
                let usage = message.usage.clone();
                let text = message.content.unwrap_or_default();
                let mut meter = super::meter(&usage);
                meter.live_millis = Some(super::elapsed_millis(started));
                self.session
                    .finish_judge(attempt, &text, meter)
                    .map_err(super::session_err)?;
                self.spent_tokens = self.spent_tokens.saturating_add(u64::from(
                    usage.prompt_tokens.saturating_add(usage.completion_tokens),
                ));
                Ok((text, usage))
            }
            Err(error) => {
                self.session
                    .fail_provider(
                        attempt,
                        super::durable_failure(error.provider_failure()),
                        &error.to_string(),
                        super::elapsed_millis(started),
                    )
                    .map_err(super::session_err)?;
                self.spent_tokens = self.session.replay().spent_tokens;
                Err(error)
            }
        }
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

    /// Remove the current conversation while keeping the provider and tools.
    pub fn clear_history(&mut self) -> Result<()> {
        self.session.clear().map_err(super::session_err)?;
        self.history.clear();
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

    /// Start or resume numbered-step Work with every check declared by its
    /// plan and contract.
    pub fn select_steps(&mut self, markdown: &str, commands: &[String]) -> Result<()> {
        self.session
            .prepare_external_work(
                markdown,
                markdown,
                crate::durable::work::WorkPolicy::Steps,
                commands,
            )
            .map_err(super::session_err)?;
        Ok(())
    }

    /// Start or resume the durable Work owned by the standing-goal runner.
    pub fn select_goal(
        &mut self,
        objective: &str,
        commands: &[String],
        max_turns: usize,
    ) -> Result<()> {
        let max_turns = u32::try_from(max_turns)
            .map_err(|_error| Error::Config("goal turn limit is too large".to_string()))?;
        self.session
            .prepare_external_work(
                objective,
                objective,
                crate::durable::work::WorkPolicy::Goal { max_turns },
                commands,
            )
            .map_err(super::session_err)
    }

    /// Accept the selected interactive Plan and enter Execute.
    pub fn accept_plan(&mut self) -> Result<()> {
        self.session.accept_plan().map_err(super::session_err)
    }

    /// Whether a journal contains this workspace's active Steps Work.
    #[must_use]
    pub fn steps_at(path: &Path, workspace: &Path, markdown: &str) -> bool {
        crate::durable::product::work_matches(
            path,
            workspace,
            crate::durable::work::WorkPolicy::Steps,
            markdown,
        )
    }

    /// Whether a journal contains this workspace's active Goal Work.
    #[must_use]
    pub fn goal_at(path: &Path, workspace: &Path, objective: &str, max_turns: usize) -> bool {
        let Ok(max_turns) = u32::try_from(max_turns) else {
            return false;
        };
        crate::durable::product::work_matches(
            path,
            workspace,
            crate::durable::work::WorkPolicy::Goal { max_turns },
            objective,
        )
    }
}

fn restored_start(elapsed_millis: u64) -> std::time::Instant {
    std::time::Instant::now()
        .checked_sub(std::time::Duration::from_millis(elapsed_millis))
        .unwrap_or_else(std::time::Instant::now)
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
    Err(Error::Config(
        "session path is not a format-2 journal".to_string(),
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
