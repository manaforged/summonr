use super::*;

impl SessionDriver {
    pub(crate) fn enter_verify(&mut self) -> Result<Vec<String>, DriverError> {
        if self.turn.is_none() {
            return Err(DriverError::NoTurn);
        }
        let unchanged = self.unchanged_turn();
        self.settle_base()?;
        let base = match self.work.as_ref().and_then(|work| work.base.clone()) {
            Some(base) => base,
            None => {
                let now = self.snapshot(None)?;
                self.work.as_mut().ok_or(DriverError::NoWork)?.base = Some(now.clone());
                now
            }
        };
        let capture = if unchanged {
            base
        } else {
            self.snapshot(Some(&base))?
        };
        let paths = self
            .work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .verify(capture)?;
        self.checkpoint()?;
        Ok(paths)
    }

    pub(crate) fn prepare_verdict(&mut self, passed: bool) -> Result<Verdict, DriverError> {
        let candidate = self
            .work
            .as_ref()
            .and_then(|work| work.candidate.as_ref())
            .map(|candidate| candidate.snapshot.clone());
        let now = self.snapshot(candidate.as_ref())?;
        let stable = candidate.is_some_and(|candidate| candidate == now);
        Ok(Verdict {
            passed: passed && stable,
            diagnostic: (!stable).then(|| "workspace changed after candidate capture".to_string()),
        })
    }

    pub(crate) fn captured_verdict(&self, passed: bool) -> Verdict {
        Verdict {
            passed,
            diagnostic: None,
        }
    }

    pub(crate) fn enter_review(&mut self) -> Result<(), DriverError> {
        if self.turn.is_none() {
            return Err(DriverError::NoTurn);
        }
        self.work.as_mut().ok_or(DriverError::NoWork)?.review()?;
        self.checkpoint()
    }

    pub(crate) fn commit_review(
        &mut self,
        findings: &str,
        receipt: Option<&crate::verify::Receipt>,
    ) -> Result<(), DriverError> {
        if self.turn.is_none() {
            return Err(DriverError::NoTurn);
        }
        self.work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .reject(findings)?;
        self.close_with(receipt)
    }

    pub(crate) fn commit_verdict(
        &mut self,
        verdict: Verdict,
        decision: Decision,
        receipt: Option<&crate::verify::Receipt>,
    ) -> Result<(), DriverError> {
        if self.turn.is_none() {
            return Err(DriverError::NoTurn);
        }
        self.work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .verdict(verdict.passed, decision)?;
        self.close_with(receipt)
    }

    fn close_with(&mut self, receipt: Option<&crate::verify::Receipt>) -> Result<(), DriverError> {
        self.checkpoint()?;
        if let Some(receipt) = receipt {
            self.append(Line::Receipt {
                text: String::new(),
                receipt: Some(receipt.clone()),
            })?;
        }
        self.close()
    }
}
