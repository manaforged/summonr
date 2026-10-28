use super::*;

impl<P: Provider> Loop<P> {
    pub(super) async fn run_verify(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Vec<Outcome>> {
        let declared = self.session.completion_checks()?;
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
                    let result = self.tools.run_check(&command, None).await;
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
                    let result = self.tools.run_check(&command, None).await;
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

    pub async fn stop_work(&mut self, reason: &str, mut emit: impl FnMut(&Event)) -> Result<i32> {
        if !self.work_progress()?.terminal() {
            let checks = self.run_verify(&mut Vec::new(), &mut emit).await?;
            if !checks.is_empty() {
                self.session
                    .record_check(checks.iter().all(|check| check.passed))?;
            }
        }
        self.abandon_work(reason)?;
        let progress = self.work_progress()?;
        emit(&Event::Work {
            progress: progress.clone(),
        });
        Ok(progress.exit_code().unwrap_or(3))
    }

    fn restore_check_file(&self) {
        let path = crate::verify::check_path(&self.workspace);
        drop(match &self.check_file {
            Some(bytes) => std::fs::write(path, bytes),
            None => std::fs::remove_file(path),
        });
    }

    fn check_file_drifted(&self) -> bool {
        self.verify.is_some()
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
        let receipt = crate::verify::aborted(snapshot);
        drop(self.kill_spawned(&abort.reason, usage).await?);
        self.tools.stop_jobs()?;
        self.session.abort(&abort, Some(&receipt))?;
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
                "Harness: the previous turn stopped early ({reason})."
            ));
        }
        parts.push(harness_section(&Harness {
            mode: self.mode,
            checks: self.verify.as_deref().unwrap_or_default(),
            children: self.task_available(),
        }));
        if self.is_goal() && !progress.objective.is_empty() {
            parts.push(format!(
                "Harness objective (verbatim, this is what must be met):\n{}",
                progress.objective
            ));
        }
        if let Some((metric, _)) = self.goal_metric() {
            parts.push(metric.rules());
        }
        if progress.kind != WorkKind::Single && !progress.ledger.is_empty() {
            parts.push(format!(
                "{}\n{}",
                crate::durable::work::Work::ledger_legend(progress.kind),
                progress.ledger.join("\n")
            ));
        }
        let mut status = Vec::new();
        if !progress.todos.is_empty() {
            let mut done = 0;
            let mut pending = 0;
            let mut completed = None;
            for todo in &progress.todos {
                match todo.status {
                    crate::tools::TodoStatus::Active => {
                        status.push(format!("Current: {}", todo.text))
                    }
                    crate::tools::TodoStatus::Done => {
                        done += 1;
                        completed = Some(&todo.text);
                    }
                    crate::tools::TodoStatus::Pending => pending += 1,
                }
            }
            if let Some(completed) = completed {
                status.push(format!("Last completed: {completed}"));
            }
            status.push(format!("Tasks: {done} done, {pending} pending. Update todo on status changes; done is not a passing check."));
        }
        let mut job_failure = false;
        let mut job_success = false;
        let mut completed = std::collections::HashSet::new();
        for job in crate::jobs::since(self.workspace(), 0, Some(&self.session_id()))
            .into_iter()
            .rev()
        {
            let Ok(report) = crate::jobs::status(self.workspace(), &job) else {
                continue;
            };
            if !matches!(report.state, crate::jobs::JobState::Running)
                && !completed.insert(job.command.clone())
            {
                continue;
            }
            match report.state {
                crate::jobs::JobState::Running => {
                    status.push(format!("Running job {}: {}", job.id, job.command))
                }
                crate::jobs::JobState::Exited(0) if !job_success => {
                    status.push(format!("Latest successful job {}: {}", job.id, job.command));
                    job_success = true;
                }
                crate::jobs::JobState::Exited(0) => {}
                crate::jobs::JobState::Exited(_)
                | crate::jobs::JobState::Killed
                | crate::jobs::JobState::Dead
                    if !job_failure =>
                {
                    status.push(format!(
                        "Unresolved job failure {} ({:?}): {}",
                        job.id, report.state, job.command
                    ));
                    job_failure = true;
                }
                crate::jobs::JobState::Exited(_)
                | crate::jobs::JobState::Killed
                | crate::jobs::JobState::Dead => {}
            }
        }
        if let Some(failed) = self
            .session
            .history()
            .iter()
            .rev()
            .find(|message| message.role == crate::provider::Role::Tool)
            .filter(|message| message.failed)
        {
            let text = failed
                .content
                .lines()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("tool returned a failure");
            status.push(format!(
                "Latest tool failure: {}",
                crate::display::ellipsize(text, crate::limits::limits().output_line_bytes)
            ));
        }
        if !status.is_empty() {
            parts.push(format!("Harness progress:\n{}", status.join("\n")));
        }
        if self.role_model(workflow::Role::Review).is_some() {
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
