use super::*;

impl SessionDriver {
    pub(crate) fn enter_verify(&mut self) -> Result<Vec<String>, DriverError> {
        if self.turn.is_none() {
            return Err(DriverError::NoTurn);
        }
        let previous = self.work.as_ref().and_then(|work| work.base.as_ref());
        let capture = self.snapshot(previous)?;
        let paths = self
            .work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .verify(capture)?;
        self.checkpoint()?;
        Ok(paths)
    }

    pub(crate) fn prepare_verdict(&self, passed: bool) -> Result<Verdict, DriverError> {
        let candidate = self
            .work
            .as_ref()
            .and_then(|work| work.candidate.as_ref())
            .map(|candidate| &candidate.snapshot);
        let now = self.snapshot(candidate)?;
        let stable = candidate.is_some_and(|candidate| *candidate == now);
        if stable {
            return Ok(Verdict {
                passed,
                diagnostic: None,
            });
        }
        Ok(Verdict {
            passed: false,
            diagnostic: Some("workspace changed after candidate capture".to_string()),
        })
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
        self.checkpoint()?;
        if let Some(receipt) = receipt {
            self.append(Line::Receipt {
                text: String::new(),
                receipt: Some(receipt.clone()),
            })?;
        }
        self.close()
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
