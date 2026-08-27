use super::*;
use crate::durable::driver::work_at;
use crate::durable::log;
use crate::durable::work::Policy as WorkPolicy;
use crate::workflow;

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

    /// Resume the session whose log is at `path`.
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

    /// Copy the session at `source` into a new child and continue there.
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

    /// Create a hermetic loop. The session lives under `workspace/sessions`.
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

    /// Seconds of provider and tool work charged to this session. Idle time
    /// never counts, and the tally carries across resume, so the budget means
    /// the same thing whether the session is live or resumed.
    #[must_use]
    pub fn elapsed_secs(&self) -> u64 {
        self.session.replay().elapsed_millis / 1000
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

    /// Give `role` its own seat. Roles without one run on the driving seat.
    pub fn set_seat(&mut self, role: workflow::Role, provider: P, model: impl Into<String>) {
        self.seats.insert(role, (provider, model.into()));
    }

    /// Return `role` to the driving seat.
    pub fn clear_seat(&mut self, role: workflow::Role) {
        self.seats.remove(&role);
    }

    /// The model `role` runs on when it has its own seat.
    #[must_use]
    pub fn seat_model(&self, role: workflow::Role) -> Option<&str> {
        self.seats.get(&role).map(|(_, model)| model.as_str())
    }

    /// Record the active workflow name in the session; `None` clears it.
    /// Seats are connected by the caller.
    pub fn set_workflow(&mut self, name: Option<&str>) -> Result<()> {
        self.session.set_workflow(name).map_err(super::session_err)
    }

    /// The workflow name the session recorded, if any.
    #[must_use]
    pub fn workflow(&self) -> Option<String> {
        self.session.workflow().map(str::to_string)
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

    /// Path of the session log.
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

    /// The live Goal Work's objective and progress, when one is active.
    pub fn active_goal(&self) -> Result<Option<(String, crate::WorkProgress)>> {
        let Some(objective) = self.session.goal_objective().map_err(super::session_err)? else {
            return Ok(None);
        };
        let progress = self.work_progress()?;
        Ok((!progress.terminal()).then_some((objective, progress)))
    }

    /// One human-readable status block for the active goal, if any.
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
        self
    }

    /// Fork this session into a child log and continue on the child.
    pub fn fork_live(&mut self) -> Result<String> {
        let path = self.session.fork().map_err(super::session_err)?;
        let child = resume_driver(&self.workspace, &path)?;
        let replay = child.replay();
        let id = child.id().to_string();
        self.adopt(child, replay);
        Ok(id)
    }

    /// Swap the live session for a fork: new log, replayed history, and
    /// a fresh bus identity. The old session file stays untouched.
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

    /// Run the Goal judge as a durable, metered provider effect.
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
        self.session.abort(reason).map_err(super::session_err)
    }

    /// Start or resume numbered-step Work with every check declared by its
    /// plan and contract.
    pub fn select_steps(&mut self, markdown: &str, commands: &[String]) -> Result<()> {
        self.session
            .prepare_external_work(markdown, WorkPolicy::Steps, commands)
            .map_err(super::session_err)
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
            .prepare_external_work(objective, WorkPolicy::Goal { max_turns }, commands)
            .map_err(super::session_err)
    }

    /// Accept the selected interactive Plan and enter Execute.
    pub fn accept_plan(&mut self) -> Result<()> {
        self.session.accept_plan().map_err(super::session_err)
    }

    /// Whether the session at `path` holds this workspace's active Steps Work.
    #[must_use]
    pub fn steps_at(path: &Path, workspace: &Path, markdown: &str) -> bool {
        work_at(path, workspace, WorkPolicy::Steps, markdown)
    }

    /// Whether the session at `path` holds this workspace's active Goal Work.
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

/// Clip one line of status text to `max` characters on a char boundary.
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
