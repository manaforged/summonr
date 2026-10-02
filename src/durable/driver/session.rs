use super::*;

impl SessionDriver {
    pub(crate) fn open(
        home: &Path,
        workspace: &Path,
        model: &str,
        system: &str,
        parent: Option<SessionId>,
    ) -> Result<Self, DriverError> {
        let id = SessionId::new();
        let path = log::session_path(home, id);
        let mut driver = Self {
            partial: Partial::default(),
            log: Log::create(&path)?,
            state: State::default(),
            work: None,
            workspace: canonical(workspace),
            turn: None,
            next: 1,
            pending: Vec::new(),
            saved: AtomicU64::new(0),
        };
        driver.append(Line::Session {
            id,
            model: model.to_string(),
            system: system.to_string(),
            workspace: driver.workspace.clone(),
            parent,
            depth: u8::from(parent.is_some()),
        })?;
        Ok(driver)
    }

    pub(crate) fn resume(
        home: &Path,
        id: SessionId,
        workspace: &Path,
    ) -> Result<Self, DriverError> {
        let caller = workspace;
        let path = log::session_path(home, id);
        let mut state = State::default();
        for line in log::read(&path)? {
            state.apply(line);
        }
        if state.id != Some(id) {
            return Err(DriverError::State(format!(
                "{} is not the log of session {id}",
                path.display()
            )));
        }
        let workspace = if state.workspace.as_os_str().is_empty() {
            canonical(workspace)
        } else {
            state.workspace.clone()
        };
        let work = Work::load(&log::work_path(&path))?;
        let workspace = if workspace.is_dir() {
            workspace
        } else if work.as_ref().is_none_or(Work::terminal) {
            canonical(caller)
        } else {
            return Err(DriverError::State(format!(
                "session {id} was recorded in {}, which no longer exists",
                workspace.display()
            )));
        };
        let mut driver = Self {
            partial: Partial::default(),
            log: Log::open(&path)?,
            work,
            state,
            workspace,
            turn: None,
            next: 1,
            pending: Vec::new(),
            saved: AtomicU64::new(0),
        };
        driver.recover()?;
        Ok(driver)
    }

    fn recover(&mut self) -> Result<(), DriverError> {
        if self.state.opener.is_some() {
            self.turn = Some(Turn {
                stage: self.work.as_ref().and_then(Work::stage),
                round: None,
                calls: Vec::new(),
            });
            self.abort("process stopped before the turn finished", None)?;
            if let Some(work) = &mut self.work
                && !work.terminal()
            {
                work.pending.push(
                    "[summon: the previous attempt at this turn was interrupted; the workspace is as it left it]"
                        .to_string(),
                );
                self.checkpoint()?;
            }
            return Ok(());
        }
        if let Some(work) = &mut self.work
            && work.interrupt()
        {
            self.checkpoint()?;
        }
        self.interrupt_children("process stopped before the child finished")?;
        Ok(())
    }
}
