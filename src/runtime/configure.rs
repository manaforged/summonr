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
        loop_.join_mail();
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
        loop_.join_mail();
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
        let config = crate::config::file()?;
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
            summary_wait: 0,
            workspace: workspace
                .canonicalize()
                .unwrap_or_else(|_| workspace.to_path_buf()),
            mailbox: None,
            mail_module: false,
            mcp: None,
            guard: super::guard::Guard::configured(&config.guard),
            checkpoint_rounds: config
                .guard
                .checkpoint_rounds
                .unwrap_or(crate::limits::CHECKPOINT_ROUNDS),
            checkpoints: None,
            fallbacks: VecDeque::new(),
            compact_tokens: None,
            hooks: config.hooks,
            mcp_start: None,
            inbox: std::collections::VecDeque::new(),
            steer: std::collections::VecDeque::new(),
            steer_shared: None,
            depth,
            tasks: true,
            plan_sandbox: false,
            interactive: false,
            asked: Vec::new(),
            spawned: BTreeMap::new(),
            events: tokio::sync::mpsc::unbounded_channel(),
            catalog_frozen: false,
        })
    }

    pub(crate) fn steer_with(
        &mut self,
        source: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<UserInput>>>,
    ) {
        self.steer_shared = Some(source);
        self.interactive = true;
    }

    #[must_use]
    pub fn with_interactive(mut self) -> Self {
        self.interactive = true;
        self
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
        self.mail_module = switches.enabled("mail");
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

    #[must_use]
    pub fn policy_inert(&self) -> bool {
        self.tools.policy_inert()
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
        if !plan && std::mem::take(&mut self.plan_sandbox) {
            self.tools.disable_sandbox();
        }
        self.plan_sandbox |= matches!(sandbox, Some(Ok(())));
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
}

mod seat;
mod work;

fn resume_driver(workspace: &Path, path: &Path) -> Result<SessionDriver> {
    let (Some(home), Some(session)) = (log::home_of(path), log::parse_path(path)) else {
        return Err(Error::Config(format!(
            "{} is not a session log",
            path.display()
        )));
    };
    SessionDriver::resume(home, session, workspace).map_err(super::session_err)
}

pub(super) fn open_driver(
    home: impl AsRef<Path>,
    workspace: &Path,
    model: &str,
    system: &str,
) -> Result<SessionDriver> {
    SessionDriver::open(home.as_ref(), workspace, model, system).map_err(super::session_err)
}
