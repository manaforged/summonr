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
        let excludes = self.excludes();
        let cache = self.tree_cache();
        self.base = Some(std::thread::spawn(move || {
            let previous = previous.or_else(|| {
                cache
                    .as_deref()
                    .and_then(|cache| tree::remembered(cache, &workspace))
            });
            tree::snapshot(&workspace, &excludes, previous.as_ref())
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
