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
        let session = session_home::fresh(
            crate::prompt::summonr_home(),
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
        let run_base = (session.spent_tokens(), session.elapsed_millis());
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
            tools,
            schema: None,

            herdr,
            session,
            verify: None,
            check_file: None,
            budget: Budget {
                max_tokens,
                max_secs: max_live_millis
                    .map(|millis| millis.saturating_add(999).saturating_div(1_000)),
            },
            spent_tokens: 0,
            run_base,
            context_sample: None,
            context_window: None,
            probe: None,
            effort: None,
            turn_started: 0,
            summary_wait: 0,
            workspace: dunce::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf()),
            mailbox: None,
            mail_module: false,
            mcp: None,
            checkpoints: None,
            fallbacks: VecDeque::new(),
            compact_tokens: None,
            hooks: config
                .hooks
                .clone()
                .with_plugins(&crate::plugins::enabled()),
            mcp_wanted: false,
            mcp_board: crate::mcp::Board::default(),
            inbox: std::collections::VecDeque::new(),
            steer: std::collections::VecDeque::new(),
            steer_shared: None,
            depth,
            tasks: true,
            interactive: false,
            spawned: BTreeMap::new(),
            events: tokio::sync::mpsc::unbounded_channel(),
            catalog_frozen: false,
        })
    }

    pub(crate) fn steer_with(&mut self, source: std::sync::Arc<super::turn::Steering>) {
        self.steer_shared = Some(source);
        self.interactive = true;
    }

    #[must_use]
    pub fn with_interactive(mut self) -> Self {
        self.interactive = true;
        self
    }

    pub fn set_modules(&mut self, switches: &crate::modules::Switches) {
        use crate::modules::ModuleId;
        if self.catalog_frozen {
            return;
        }
        self.tools.set_modules(switches);
        self.tasks = switches.enabled(ModuleId::Task);
        self.mail_module = switches.enabled(ModuleId::Mail);
        self.switch_mcp(switches.enabled(ModuleId::Mcp));
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

    pub(crate) fn toggle_approval(&mut self) -> bool {
        self.tools.toggle_approval()
    }

    pub(crate) fn is_approval(&self) -> bool {
        self.tools.policy().is_approval()
    }

    pub fn set_images(&mut self, model_images: bool, vision: Option<crate::endpoint::Endpoint>) {
        self.tools.set_images(model_images, vision);
    }

    pub fn with_sandbox(mut self, network: crate::Network) -> Result<Self> {
        self.tools.enable_sandbox(network)?;
        Ok(self)
    }

    pub(crate) fn set_network(&mut self, network: crate::Network) -> Result<()> {
        if self.tools.has_sandbox() {
            self.tools.enable_sandbox(network)?;
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

    pub(crate) fn set_mode(&mut self, mode: crate::RunMode) {
        self.mode = mode;
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
        self.session
            .elapsed_millis()
            .saturating_sub(self.run_base.1)
            / 1000
    }

    #[must_use]
    pub(crate) fn run_tokens(&self) -> u64 {
        self.spent_tokens.saturating_sub(self.run_base.0)
    }

    #[must_use]
    pub(crate) fn is_strict(&self) -> bool {
        self.tools.is_strict()
    }
}

mod seat;
mod session_home;
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
    SessionDriver::open(home.as_ref(), workspace, model, system, None).map_err(super::session_err)
}
