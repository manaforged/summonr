use super::*;

pub(super) type Pending = std::thread::JoinHandle<std::io::Result<Snapshot>>;

impl SessionDriver {
    pub(super) fn begin_base(&mut self) {
        drop(self.settle_pending());
        let previous = self
            .work
            .as_ref()
            .and_then(|work| {
                work.candidate
                    .as_ref()
                    .map(|candidate| &candidate.snapshot)
                    .or(work.base.as_ref())
            })
            .or(self.last.as_ref())
            .cloned();
        let workspace = self.workspace.clone();
        let sessions = self.sessions_dir();
        self.base = Some(std::thread::spawn(move || {
            let previous = previous.or_else(|| tree::remembered(&workspace));
            let snapshot = tree::snapshot(&workspace, sessions.as_deref(), previous.as_ref())?;
            tree::remember(&workspace, &snapshot);
            Ok(snapshot)
        }));
        self.fresh = true;
    }

    pub(crate) fn settle_base(&mut self) -> Result<(), DriverError> {
        if self.settle_pending()? {
            self.checkpoint()?;
        }
        Ok(())
    }

    pub(super) fn unchanged_turn(&self) -> bool {
        self.fresh && self.turn.is_some()
    }

    fn settle_pending(&mut self) -> Result<bool, DriverError> {
        let Some(task) = self.base.take() else {
            return Ok(false);
        };
        let base = task
            .join()
            .unwrap_or_else(|payload| std::panic::resume_unwind(payload))?;
        self.last = Some(base.clone());
        if let Some(work) = self.work.as_mut() {
            work.base = Some(base);
        }
        Ok(true)
    }
}
