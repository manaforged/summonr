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
        if self.check_file_drifted() {
            self.restore_check_file();
            self.push_event(
                events,
                emit,
                Event::Note {
                    text: "the check file changed during this run; summonr put it back and verified with the check it loaded at start".to_string(),
                },
            )?;
        }
        let restored = self.restore_protected();
        if !restored.is_empty() {
            self.push_event(
                events,
                emit,
                Event::Note {
                    text: format!(
                        "summonr put back {}: files the check and score commands use are read-only while the goal runs",
                        restored.join(", ")
                    ),
                },
            )?;
        }
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
                    let result = self.tools.run_check(&command).await;
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
                    let result = self.tools.run_check(&command).await;
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

    pub(crate) fn goal_score_config(&self) -> Result<Option<crate::verify::ScoreConfig>> {
        if self
            .session
            .goal_objective()
            .map_err(super::session_err)?
            .is_none()
        {
            return Ok(None);
        }
        match &self.check_file {
            Some(bytes) => crate::verify::score_config(&self.workspace, bytes),
            None => Ok(None),
        }
    }

    pub(crate) fn goal_board(&self) -> Option<crate::score::Board> {
        let config = self.goal_score_config().ok().flatten()?;
        let session = self.session.id().to_string();
        crate::score::board(&self.workspace, &config, Some(&session), &self.goal_score).ok()
    }

    pub(super) async fn score_goal(
        &mut self,
        watched: Option<crate::score::Spend>,
    ) -> Result<Option<crate::score::Measured>> {
        let Some(config) = self.goal_score_config()? else {
            return Ok(None);
        };
        let session = Some(self.session.id().to_string());
        crate::score::measure(
            &self.tools,
            &self.workspace,
            &config,
            session,
            &mut self.goal_score,
            watched,
        )
        .await
        .map(Some)
    }

    pub(super) fn budget_stop(
        &mut self,
        scored: &crate::score::Measured,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<bool> {
        let Some(spend) = scored
            .board()
            .spend
            .as_ref()
            .filter(|spend| spend.breached())
        else {
            return Ok(false);
        };
        let text = spend.note();
        self.push_event(events, emit, Event::Note { text })?;
        for text in self.tools.stop_jobs()? {
            self.push_event(events, emit, Event::Note { text })?;
        }
        Ok(true)
    }

    pub(super) async fn watch_spend(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Option<crate::score::Spend>> {
        let Some(config) = self.goal_score_config()? else {
            return Ok(None);
        };
        let session = self.session.id().to_string();
        let spend = crate::score::watch(
            &self.tools,
            &self.workspace,
            &config,
            Some(&session),
            &mut self.goal_score,
        )
        .await?;
        if let Some(spend) = &spend {
            let text = spend.line();
            self.push_event(events, emit, Event::Note { text })?;
        }
        Ok(spend.filter(crate::score::Spend::breached))
    }

    pub async fn stop_work(&mut self, reason: &str, mut emit: impl FnMut(&Event)) -> Result<i32> {
        if !self.work_progress()?.terminal() {
            let checks = self.run_verify(&mut Vec::new(), &mut emit).await?;
            if !checks.is_empty() {
                self.session
                    .record_check(checks.iter().all(|check| check.passed))
                    .map_err(super::session_err)?;
            }
        }
        self.abandon_work(reason)?;
        let progress = self.work_progress()?;
        emit(&Event::Work {
            progress: progress.clone(),
        });
        Ok(progress.exit_code().unwrap_or(3))
    }

    pub(crate) fn snapshot_check_file(&mut self) {
        self.check_file = std::fs::read(crate::verify::check_path(&self.workspace)).ok();
        self.protected = self
            .check_file
            .as_deref()
            .map(|bytes| crate::verify::protected(&self.workspace, bytes))
            .unwrap_or_default()
            .into_iter()
            .filter_map(|path| std::fs::read(&path).ok().map(|bytes| (path, bytes)))
            .collect();
    }

    fn restore_protected(&self) -> Vec<String> {
        let mut restored = Vec::new();
        for (path, bytes) in &self.protected {
            if std::fs::read(path).ok().as_ref() == Some(bytes) {
                continue;
            }
            if let Some(parent) = path.parent() {
                drop(std::fs::create_dir_all(parent));
            }
            if std::fs::write(path, bytes).is_ok() {
                let shown = path.strip_prefix(&self.workspace).unwrap_or(path);
                restored.push(shown.display().to_string());
            }
        }
        restored
    }

    fn restore_check_file(&self) {
        let path = crate::verify::check_path(&self.workspace);
        drop(match &self.check_file {
            Some(bytes) => std::fs::write(path, bytes),
            None => std::fs::remove_file(path),
        });
    }

    fn check_file_drifted(&self) -> bool {
        (self.verify.is_some() || self.check_file.is_some())
            && std::fs::read(crate::verify::check_path(&self.workspace)).ok() != self.check_file
    }

    pub(super) async fn abort_turn(
        &mut self,
        abort: Abort,
        events: &mut Vec<Event>,
        mut emit: impl FnMut(&Event),
        usage: &mut Usage,
        reached: usize,
        pre: &mut Pretask,
    ) -> Result<Turn> {
        let snapshot = self.settle(pre, events, &mut emit).await?;
        let receipt = Receipt::aborted(snapshot, events);
        drop(self.kill_spawned(&abort.reason, usage).await?);
        for text in self.tools.stop_jobs()? {
            self.push_event(events, &mut emit, Event::Note { text })?;
        }
        self.session
            .abort(&abort, Some(&receipt))
            .map_err(super::session_err)?;
        self.emit_records(events, &mut emit)?;
        self.sync_usage(&mut emit);
        Ok(Turn {
            text: abort.reason,
            events: std::mem::take(events),
            rounds: reached,
            usage: std::mem::take(usage),
            checks: Vec::new(),
            aborted: true,
        })
    }

    pub(super) fn harness_note(&self, progress: &WorkProgress) -> String {
        let mut parts = Vec::new();
        if let Some(reason) = self.session.last_close_reason() {
            parts.push(format!(
                "Harness: the previous turn stopped early ({reason}). Address the user's \
                 message directly before resuming tool work."
            ));
        }
        parts.push(harness_section(&Harness {
            mode: self.mode,
            checks: self.verify.as_deref().unwrap_or_default(),
            children: self.task_available(),
        }));
        if progress.kind == WorkKind::Goal {
            parts.push(format!(
                "Harness objective (verbatim, this is what must be met):\n{}",
                progress.objective
            ));
        }
        if progress.kind != WorkKind::Single && !progress.ledger.is_empty() {
            parts.push(format!(
                "{}\n{}",
                crate::durable::work::Work::ledger_legend(),
                progress.ledger.join("\n")
            ));
        }
        if !progress.todos.is_empty() {
            let tasks: Vec<String> = progress.todos.iter().map(|todo| todo.line()).collect();
            parts.push(format!(
                "Harness task list (update with `todo` as work starts or finishes; a done item is not a passing check):\n{}",
                tasks.join("\n")
            ));
        }
        if self.seat_model(workflow::Role::Review).is_some() {
            parts.push(
                "Harness: a reviewer reads the change after the completion check and can send it back."
                    .to_string(),
            );
        }
        parts.retain(|part| !part.is_empty());
        parts.join("\n\n")
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
