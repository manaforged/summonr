use super::*;

impl SessionDriver {
    /// Captures the workspace for Verify; `wrote` names the paths this turn's tools changed.
    pub(crate) fn enter_verify(&mut self, wrote: &[String]) -> Result<Vec<String>> {
        if self.turn.is_none() {
            return Err(no_turn());
        }
        let unchanged = self.unchanged_turn();
        self.settle_base()?;
        let base = match self.work.as_ref().and_then(|work| work.base.clone()) {
            Some(base) => base,
            None => {
                let now = self.snapshot(None, &[])?;
                self.work.as_mut().ok_or_else(no_work)?.base = Some(now.clone());
                now
            }
        };
        let capture = if unchanged {
            base
        } else {
            self.snapshot(Some(&base), wrote)?
        };
        let paths = self.work.as_mut().ok_or_else(no_work)?.verify(capture)?;
        self.checkpoint()?;
        Ok(paths)
    }

    pub(crate) fn limit_changes(&mut self, wrote: &[String]) -> Result<Vec<String>> {
        let work = self.work.as_mut().ok_or_else(no_work)?;
        let kept: std::collections::HashSet<&str> = wrote.iter().map(String::as_str).collect();
        if let Some(candidate) = work.candidate.as_mut() {
            candidate.paths.retain(|path| kept.contains(path.as_str()));
        }
        work.touched.retain(|path| kept.contains(path.as_str()));
        Ok(wrote.to_vec())
    }

    pub(crate) fn prepare_verdict(&mut self, passed: bool, wrote: &[String]) -> Result<Verdict> {
        let candidate = self
            .work
            .as_ref()
            .and_then(|work| work.candidate.as_ref())
            .map(|candidate| candidate.snapshot.clone());
        let now = self.snapshot(candidate.as_ref(), wrote)?;
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

    pub(crate) fn commit_verdict(
        &mut self,
        verdict: Verdict,
        decision: Decision,
        receipt: Option<&crate::verify::Receipt>,
    ) -> Result<()> {
        if self.turn.is_none() {
            return Err(no_turn());
        }
        let work = self.work.as_mut().ok_or_else(no_work)?;
        let failure = receipt
            .filter(|receipt| !verdict.passed && !receipt.checks.is_empty())
            .map(|receipt| receipt.checks.clone());
        let denied = failure.as_ref().is_some_and(|checks| {
            checks
                .iter()
                .any(|check| !check.passed && check.output.starts_with("policy denied"))
        });
        if denied {
            work.record_check(false);
            work.abandon("completion check denied by policy");
        } else if matches!(work.policy, WorkKind::Goal)
            && work.measurements.is_empty()
            && failure.is_some()
            && failure == work.failed_checks
        {
            work.record_check(false);
            work.abandon("completion check failed with the same output twice");
        } else {
            work.verdict(verdict.passed, decision)?;
        }
        work.failed_checks = failure;
        self.close_with(receipt)
    }

    fn close_with(&mut self, receipt: Option<&crate::verify::Receipt>) -> Result<()> {
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
