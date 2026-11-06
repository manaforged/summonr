use super::*;

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
        self.base = Some(tree::base::begin(
            self.workspace.clone(),
            self.log.path(),
            previous,
        ));
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
        let base = tree::base::settle(task)?;
        self.last = Some(base.clone());
        if let Some(work) = self.work.as_mut() {
            work.base = Some(base);
        }
        Ok(true)
    }
}
