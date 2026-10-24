use super::*;

impl<P: Provider> Loop<P> {
    pub(crate) fn active_goal(&self) -> Result<Option<(String, crate::WorkProgress)>> {
        let Some(objective) = self.session.goal_objective().map_err(super::session_err)? else {
            return Ok(None);
        };
        let progress = self.work_progress()?;
        Ok((!progress.terminal()).then_some((objective, progress)))
    }

    #[must_use]
    pub(crate) fn goal_status(&self) -> Option<String> {
        let (objective, progress) = self.active_goal().ok().flatten()?;
        let round = progress.rounds;
        let mut text = format!(
            "goal active: {}\nround {round} · {} tokens · {}",
            crate::text::ellipsize(objective.lines().next().unwrap_or_default(), 80),
            self.spent_tokens(),
            progress.phase,
        );
        let gates = self.verify_commands();
        if !gates.is_empty() {
            text.push_str("\ngates: ");
            text.push_str(&gates.join(" then "));
        }
        if let Some(reason) = &progress.reason {
            text.push_str("\nlast: ");
            text.push_str(&crate::text::ellipsize(
                reason.lines().next().unwrap_or_default(),
                160,
            ));
        }
        Some(text)
    }

    pub fn abandon_work(&mut self, reason: &str) -> Result<()> {
        self.session
            .abandon_work(reason)
            .map_err(super::session_err)
    }

    #[must_use]
    pub(crate) fn restore_replay(mut self, replay: crate::session::Replay) -> Self {
        self.spent_tokens = replay.spent_tokens;
        self
    }

    pub(crate) fn fork_live(&mut self) -> Result<String> {
        self.fork_at(None)
    }

    pub(crate) fn fork_at(&mut self, turn: Option<usize>) -> Result<String> {
        if !self.spawned.is_empty() {
            return Err(Error::Config(
                "Collect or stop subagents before branching".to_string(),
            ));
        }
        let path = self.session.fork(turn).map_err(super::session_err)?;
        let child = resume_driver(&self.workspace, &path)?;
        let replay = child.replay();
        let id = child.id().to_string();
        self.adopt(child, replay);
        Ok(id)
    }

    pub(crate) fn adopt(&mut self, session: SessionDriver, replay: crate::session::Replay) {
        self.forget_children(crate::child::ChildScope::All);
        self.session = session;
        self.tools.set_session(&self.session.id().to_string());
        self.spent_tokens = replay.spent_tokens;
        self.mailbox = None;
        self.join_mail();
        self.schema = None;
        self.herdr.retarget(&self.session.id().to_string());
    }

    #[must_use]
    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub(crate) async fn judge(&mut self, system: &str, user: &str) -> Result<(String, Usage)> {
        let messages = [
            Message::system(system.to_string()),
            Message::user(user.to_string()),
        ];
        let request = self.request(workflow::Role::Judge, &messages, &[], false);
        match self.complete_once(workflow::Role::Judge, &request).await? {
            Ok(message) => {
                let usage = message.usage.clone();
                let text = message.content.unwrap_or_default();
                if let Some(reason) = self.round_abort() {
                    return Err(Error::Config(reason));
                }
                Ok((text, usage))
            }
            Err(wait_error) => Err(wait_error.error()),
        }
    }

    pub(crate) fn clear_history(&mut self) -> Result<()> {
        self.session.clear().map_err(super::session_err)?;
        self.tools.forget_reads();
        Ok(())
    }

    pub(crate) fn blocked_goal(&self) -> Result<Option<String>> {
        if self
            .session
            .goal_objective()
            .map_err(super::session_err)?
            .is_none()
        {
            return Ok(None);
        }
        let todos = self.work_progress()?.todos;
        let blocked: Vec<&str> = todos
            .iter()
            .filter(|todo| todo.status == crate::tools::TodoStatus::Blocked)
            .map(|todo| todo.text.as_str())
            .collect();
        let open = todos.iter().any(|todo| {
            !matches!(
                todo.status,
                crate::tools::TodoStatus::Done | crate::tools::TodoStatus::Blocked
            )
        });
        Ok((!blocked.is_empty() && !open).then(|| format!("blocked: {}", blocked.join("; "))))
    }

    pub(crate) fn goal_quiet(&self) -> usize {
        self.goal_score.quiet
    }

    pub(crate) fn goal_notes(&self) -> &[String] {
        &self.goal_score.notes
    }

    pub fn begin_goal_turn(&mut self) -> Result<bool> {
        let todos = self.work_progress()?.todos;
        let done = |todos: &[crate::tools::Todo]| {
            todos
                .iter()
                .filter(|todo| todo.status == crate::tools::TodoStatus::Done)
                .count()
        };
        let finished = self
            .goal_score
            .todos
            .as_deref()
            .is_some_and(|start| done(&todos) > done(start));
        if finished {
            self.session.clear_messages().map_err(super::session_err)?;
            self.tools.forget_reads();
        }
        self.goal_score.todos = Some(todos);
        Ok(finished)
    }

    pub(crate) fn settle_todos(
        &mut self,
        checks: &[crate::verify::Outcome],
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<()> {
        if self
            .session
            .goal_objective()
            .map_err(super::session_err)?
            .is_none()
        {
            return Ok(());
        }
        let failed = checks.iter().any(|check| !check.passed);
        let output = checks
            .iter()
            .filter(|check| !check.passed)
            .map(|check| check.output.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let check = failed.then(|| without_durations(&output));
        let unchanged = check.is_some() && check == self.goal_score.check;
        self.goal_score.check.clone_from(&check);
        let tree = crate::score::tree(&self.workspace, Some(&self.session.id().to_string()));
        let scored = self
            .goal_score_config()?
            .is_some_and(|config| config.command.is_some());
        if !scored && !checks.is_empty() {
            let reading = format!("{failed}\n{}", check.unwrap_or_default());
            self.goal_score
                .observe(tree.clone(), crate::score::Reading::Output(reading));
        }
        let Some(start) = self.goal_score.todos.clone() else {
            return Ok(());
        };
        let mut todos = self.work_progress()?.todos;
        let mut facts = Vec::new();
        if let Some((from, to)) = self.goal_score.task_tree.as_deref().zip(tree.as_deref())
            && let Ok(files) = crate::diff::between(&self.workspace, from, to)
        {
            facts.push(match files.is_empty() {
                true => "no files changed".to_string(),
                false => format!(
                    "changed {}",
                    files
                        .into_iter()
                        .map(|stat| stat.path)
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        }
        facts.push(format!(
            "the check {}",
            if failed { "failed" } else { "passed" }
        ));
        let facts = facts.join("; ");
        let mut reopened = false;
        let mut finished = false;
        for (index, todo) in todos.iter_mut().enumerate() {
            if todo.status != crate::tools::TodoStatus::Done || start.get(index) == Some(todo) {
                continue;
            }
            if unchanged && self.goal_score.reopened.insert(index) {
                todo.status = crate::tools::TodoStatus::Pending;
                reopened = true;
                let text = format!(
                    "reopened {}: the check output did not change after it was marked done",
                    todo.text
                );
                self.push_event(events, emit, Event::Note { text })?;
            } else {
                self.goal_score
                    .notes
                    .push(format!("{}: {facts}", todo.text));
                finished = true;
            }
        }
        if finished {
            self.goal_score.task_tree = tree;
        }
        if reopened {
            self.session
                .apply_todos(todos.clone())
                .map_err(super::session_err)?;
            self.goal_score.todos = Some(todos);
        }
        Ok(())
    }

    pub(crate) fn abort_note(&mut self, reason: &str) -> Result<()> {
        let abort = crate::tools::Abort {
            code: crate::tools::AbortCode::Interrupted,
            reason: reason.to_string(),
        };
        self.abort_with(&abort, |_| {})
    }

    pub(crate) fn abort_with(
        &mut self,
        abort: &crate::tools::Abort,
        mut emit: impl FnMut(&Event),
    ) -> Result<()> {
        self.forget_children(abort.code.child_scope());
        self.session
            .abort(abort, None)
            .map_err(super::session_err)?;
        let mut events = Vec::new();
        for text in self.tools.stop_jobs()? {
            self.push_event(&mut events, &mut emit, Event::Note { text })?;
        }
        self.emit_records(&mut events, &mut emit)?;
        self.sync_usage(&mut emit);
        Ok(())
    }

    pub(crate) fn unreported_tokens(&self) -> u64 {
        self.session.usage().1
    }

    fn forget_children(&mut self, scope: crate::child::ChildScope) {
        let mut ids = Vec::new();
        self.spawned.retain(|id, spawned| {
            let forget = scope == crate::child::ChildScope::All || !spawned.background();
            if forget {
                spawned.abort();
                ids.push(id.clone());
            }
            !forget
        });
        self.discard_events(&ids);
    }

    pub fn select_steps(&mut self, markdown: &str, commands: &[String]) -> Result<()> {
        self.session
            .prepare_external_work(markdown, WorkPolicy::Steps, commands)
            .map_err(super::session_err)?;
        self.schema = None;
        self.set_mode(crate::RunMode::Work);
        Ok(())
    }

    pub fn select_goal(&mut self, objective: &str, commands: &[String]) -> Result<()> {
        let fresh = self.active_goal().ok().flatten().is_none();
        self.session
            .prepare_external_work(objective, WorkPolicy::Goal {}, commands)
            .map_err(super::session_err)?;
        if fresh {
            self.goal_score = crate::score::Goal::started();
            self.goal_score.task_tree =
                crate::score::tree(&self.workspace, Some(&self.session.id().to_string()));
        }
        if fresh || self.check_file.is_none() {
            self.snapshot_check_file();
        }
        self.schema = None;
        self.set_mode(crate::RunMode::Goal);
        Ok(())
    }

    #[must_use]
    pub fn touched_at(path: &Path) -> Vec<String> {
        crate::durable::driver::touched_at(path)
    }

    #[must_use]
    pub fn job_at(path: &Path, workspace: &Path) -> Option<(crate::WorkKind, String)> {
        let (policy, objective) = crate::durable::driver::active_at(path, workspace)?;
        let kind = match policy {
            WorkPolicy::Single => crate::WorkKind::Single,
            WorkPolicy::Steps => crate::WorkKind::Steps,
            WorkPolicy::Goal { .. } => crate::WorkKind::Goal,
        };
        Some((kind, objective))
    }

    #[must_use]
    pub fn steps_at(path: &Path, workspace: &Path, markdown: &str) -> bool {
        work_at(path, workspace, WorkPolicy::Steps, markdown)
    }

    #[must_use]
    pub fn goal_at(path: &Path, workspace: &Path, objective: &str) -> bool {
        work_at(path, workspace, WorkPolicy::Goal {}, objective)
    }
}

fn without_durations(output: &str) -> String {
    regex::Regex::new(r"\d+(?:\.\d+)?\s*(?:ns|us|µs|ms|s|m|h)\b").map_or_else(
        |_| output.to_string(),
        |pattern| pattern.replace_all(output, "").into_owned(),
    )
}
