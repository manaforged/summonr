use super::*;

impl<P: Provider> Loop<P> {
    pub(super) async fn run_verify(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Vec<Outcome>> {
        let declared = self
            .session
            .completion_checks()
            .map_err(super::session_err)?;
        let mut checks = Vec::new();
        for check in declared {
            let command = check.command.clone();
            let outcome = match &check.observation {
                Some(Observation::File(path)) => match self.tools.exists(path.as_str()) {
                    Ok(true) => Outcome {
                        command,
                        passed: true,
                        output: format!("file `{}` exists", path.as_str()),
                    },
                    Ok(false) => Outcome {
                        command,
                        passed: false,
                        output: format!("file `{}` does not exist", path.as_str()),
                    },
                    Err(error) => Outcome {
                        command,
                        passed: false,
                        output: error.to_string(),
                    },
                },
                Some(Observation::Stdout(expected)) => {
                    let result = self.tools.execute_outcome(&verify_call(&command)).await;
                    let mut outcome = verify::outcome(&command, &result);
                    if outcome.passed && !outcome.output.contains(expected.as_str()) {
                        outcome.passed = false;
                        outcome.output.push_str(&format!(
                            "\nexpected stdout to contain `{}`",
                            expected.as_str()
                        ));
                    }
                    outcome
                }
                None => {
                    let result = self.tools.execute_outcome(&verify_call(&command)).await;
                    verify::outcome(&command, &result)
                }
            };
            self.push_event(
                events,
                emit,
                Event::Verify {
                    command: outcome.command.clone(),
                    passed: outcome.passed,
                },
            )?;
            let passed = outcome.passed;
            checks.push(outcome);
            if !passed {
                break;
            }
        }
        Ok(checks)
    }

    pub(super) async fn abort_turn(
        &mut self,
        abort_reason: Option<String>,
        events: &mut Vec<Event>,
        mut emit: impl FnMut(&Event),
        usage: &mut Usage,
        reached: usize,
        pre: &mut Pretask,
    ) -> Result<Turn> {
        let snapshot = self.settle(pre, events, &mut emit).await?;
        let reason = abort_reason.unwrap_or_else(|| "turn aborted".to_string());
        let receipt = Receipt::aborted(snapshot, events);
        drop(self.kill_spawned(&reason, usage).await?);
        self.tools.stop_jobs()?;
        self.session
            .abort(&reason, Some(&receipt))
            .map_err(super::session_err)?;
        self.emit_records(events, &mut emit)?;
        self.sync_usage(&mut emit);
        Ok(Turn {
            text: reason,
            events: std::mem::take(events),
            rounds: reached,
            usage: std::mem::take(usage),
            checks: Vec::new(),
            aborted: true,
        })
    }

    pub(super) fn harness_note(&self, progress: &WorkProgress) -> String {
        let mut note = String::new();
        if let Some(reason) = self.session.last_close_reason() {
            note.push_str(&format!(
                "Harness: the previous turn stopped early ({reason}) — address the user's \
                 message directly before resuming tool work.\n\n"
            ));
        }
        note.push_str(&harness_section(&Harness {
            mode: self.mode,
            checks: self.verify.as_deref().unwrap_or_default(),
            sandbox: self
                .tools
                .has_sandbox()
                .then_some(if self.denies_network() {
                    Network::Denied
                } else {
                    Network::Allowed
                }),
            children: self.task_available(),
        }));
        if progress.kind == WorkKind::Goal {
            note.push_str(&format!(
                "\n\nHarness objective (verbatim, this is what must be met):\n{}",
                progress.objective
            ));
        }
        if progress.kind != WorkKind::Single && !progress.ledger.is_empty() {
            note.push_str(
                "\n\nHarness ledger ([x] done, [/] this step, [ ] later, [!] blocked):\n",
            );
            note.push_str(&progress.ledger.join("\n"));
        }
        if self.seat_model(workflow::Role::Review).is_some() {
            note.push_str(
                "\n\nHarness: a reviewer reads the change after the completion check and can send it back.",
            );
        }
        note
    }

    pub(super) fn note_sent(&self, harness: &str) -> bool {
        self.session
            .history()
            .iter()
            .rev()
            .find(|message| message.role == crate::provider::Role::User)
            .is_some_and(|message| message.content.ends_with(harness))
    }
}
