use super::*;

impl<P: Provider> Loop<P> {
    pub(super) async fn await_jobs(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<(Vec<Outcome>, Vec<String>, Vec<String>)> {
        let mut outcomes: Vec<(String, Outcome)> = Vec::new();
        let mut running = Vec::new();
        let mut unsettled = Vec::new();
        let session = self.session.id().to_string();
        let workspace = self.workspace.clone();
        let mut owned: BTreeMap<String, jobs::Record> = BTreeMap::new();
        for id in self.session.jobs() {
            if let Ok(record) = jobs::load(&self.workspace, &id) {
                owned.insert(record.id.clone(), record);
            }
        }
        for record in jobs::since(&self.workspace, self.turn_started, Some(&session)) {
            owned.insert(record.id.clone(), record);
        }
        let deadline = Instant::now() + crate::limits::seconds(crate::limits::limits().job_wait_secs);
        for record in owned.into_values() {
            let label = format!("job {} `{}`", record.id, record.command);
            let status = jobs::wait(
                &workspace,
                &record.id,
                deadline.saturating_duration_since(Instant::now()).as_secs(),
                || {
                    self.push_event(
                        events,
                        emit,
                        Event::Note {
                            text: format!("waiting for background {label}"),
                        },
                    )
                },
            )
            .await?;
            if status.state == jobs::JobState::Killed {
                continue;
            }
            let running_now = status.state == jobs::JobState::Running;
            if running_now {
                running.push(label.clone());
                unsettled.push(record.id.clone());
            } else {
                self.tools.see_job(&record.id);
            }
            let outcome = Outcome {
                command: label,
                passed: status.state == jobs::JobState::Exited(0),
                output: if running_now {
                    format!(
                        "still running after {}s; wait for it or kill it\n{}",
                        crate::limits::limits().job_wait_secs,
                        status.text
                    )
                } else {
                    status.text
                },
            };
            match outcomes
                .iter_mut()
                .find(|(command, _)| *command == record.command)
            {
                Some((_, latest)) => *latest = outcome,
                None => outcomes.push((record.command, outcome)),
            }
        }
        for (_, outcome) in &outcomes {
            self.push_event(
                events,
                emit,
                Event::Verify {
                    command: outcome.command.clone(),
                    passed: outcome.passed,
                },
            )?;
        }
        Ok((
            outcomes.into_iter().map(|(_, outcome)| outcome).collect(),
            running,
            unsettled,
        ))
    }

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
        if let Some(outcome) = self.protected_drift() {
            self.push_event(
                events,
                emit,
                Event::Verify {
                    command: outcome.command.clone(),
                    passed: false,
                },
            )?;
            return Ok(vec![outcome]);
        }
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
        Ok(progress.exit_status(false, !progress.terminal()))
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
        self.tools.forget_approval();
        self.tools.stop_jobs()?;
        self.session.abort(&abort, Some(&receipt))?;
        self.watch_jobs();
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
                "Harness: the previous turn stopped early ({reason}). It owes nothing unless the new message asks to resume it."
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
        let open_tasks = progress
            .todos
            .iter()
            .any(|todo| todo.status != crate::tools::TodoStatus::Done);
        let lasting = progress.kind != WorkKind::Single;
        let unfinished = lasting || open_tasks || self.session.turn_open();
        if open_tasks || (lasting && !progress.todos.is_empty()) {
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
                    crate::tools::TodoStatus::Blocked => {
                        status.push(format!("Blocked: {}", todo.text))
                    }
                }
            }
            if let Some(completed) = completed {
                status.push(format!("Last completed: {completed}"));
            }
            status.push(format!("Tasks: {done} done, {pending} pending. Update todo on status changes; done is not a passing check."));
        }
        status.extend(self.uncommitted());
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
                && (!unfinished || !completed.insert(job.command.clone()))
            {
                continue;
            }
            match report.state {
                crate::jobs::JobState::Running => {
                    status.push(format!("Running job {}: {}", job.id, job_label(&job.command)))
                }
                crate::jobs::JobState::Exited(0) if !job_success && !self.job_reported(&job.id) => {
                    status.push(format!(
                        "Latest successful job {}: {}",
                        job.id,
                        job_label(&job.command)
                    ));
                    job_success = true;
                }
                crate::jobs::JobState::Exited(0) => {}
                crate::jobs::JobState::Exited(_)
                | crate::jobs::JobState::Killed
                | crate::jobs::JobState::Dead
                    if !job_failure =>
                {
                    let mut seen = self
                        .reported_jobs
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if seen.insert(job.id.clone()) {
                        status.push(format!(
                            "Unresolved job failure {} ({:?}): {}",
                            job.id,
                            report.state,
                            job_label(&job.command)
                        ));
                    }
                    job_failure = true;
                }
                crate::jobs::JobState::Exited(_)
                | crate::jobs::JobState::Killed
                | crate::jobs::JobState::Dead => {}
            }
        }
        if let Some(failed) = unfinished
            .then(|| self.session.history())
            .into_iter()
            .flatten()
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
        parts.retain(|part| !part.is_empty());
        parts.join("\n\n")
    }

    fn uncommitted(&self) -> Vec<String> {
        self.tools
            .repos()
            .into_iter()
            .filter_map(|repo| {
                let diff = crate::diff::stats(&repo).ok()?;
                (!diff.files.is_empty()).then(|| {
                    format!("Uncommitted in {}: {}", repo.display(), diff.summary())
                })
            })
            .collect()
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

/// The first line of a job command, shortened; the full command is in the job log.
fn job_label(command: &str) -> String {
    let first = command.lines().next().unwrap_or_default();
    crate::display::ellipsize(first, crate::limits::limits().output_line_bytes)
}
