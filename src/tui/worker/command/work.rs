use super::*;

impl Worker {
    pub(in crate::tui::worker) async fn compact(&mut self) -> bool {
        drop(self.ui.send(Msg::Busy(Some(Activity::Compact))));
        let ui = self.ui.clone();
        let outcome = wait_cancelable(
            &mut self.cmd,
            &mut self.queue,
            &self.reader,
            &self.ui,
            None,
            self.loop_
                .compact_with(move |event| drop(ui.send(Msg::Event(event.clone())))),
        )
        .await;
        if !matches!(outcome, Completion::Done(Ok(_))) {
            self.recover();
        }
        drop(self.ui.send(Msg::Busy(None)));
        match outcome {
            Completion::Done(Ok(true)) => {
                self.system("context compacted: earlier turns summarized")
            }
            Completion::Done(Ok(false)) => self.system("nothing to compact yet"),
            Completion::Done(Err(error)) => self.fail(&error.to_string()),
            Completion::Cancelled => {
                self.system("Compaction cancelled. The conversation is unchanged.");
            }
            Completion::Quit => return false,
        }
        true
    }

    pub(in crate::tui::worker) async fn steps(&mut self, plan_path: &str) -> bool {
        if plan_path.is_empty() {
            self.system(
                "/steps PLAN.md: run each numbered item under `## Steps`; /steps clear ends it",
            );
            return true;
        }
        if is_stop_word(plan_path) {
            self.clear_steps();
            return true;
        }
        let Some((plan, markdown, steps)) = self.read_steps(plan_path) else {
            return true;
        };
        let checks = self.loop_.verify_commands().to_vec();
        if let Err(error) = self.loop_.select_steps(&markdown, &checks) {
            self.fail(&error.to_string());
            return true;
        }
        announce_run_mode(&self.loop_, &self.ui);
        self.run_work(&crate::Job::Steps { plan, steps }).await
    }

    fn clear_steps(&mut self) {
        match self.loop_.work_progress() {
            Ok(progress) if progress.kind == crate::WorkKind::Steps && !progress.terminal() => {
                if let Err(error) = self.loop_.abandon_work("steps cleared by user") {
                    self.fail(&error.to_string());
                } else {
                    self.system("steps cleared");
                }
            }
            _ => self.system("no active steps run"),
        }
    }

    fn read_steps(&mut self, plan_path: &str) -> Option<(String, String, Vec<crate::steps::Step>)> {
        let workspace = self.loop_.workspace().to_path_buf();
        match crate::steps::load(&workspace, &workspace.join(plan_path)) {
            Ok(plan) => Some(plan),
            Err(error) => {
                self.fail(&error.to_string());
                None
            }
        }
    }

    pub(in crate::tui::worker) async fn goal(&mut self, objective: &str) -> bool {
        if objective.is_empty() {
            return self.goal_status();
        }
        if is_stop_word(objective) {
            self.clear_goal();
            self.leave_goal(crate::WorkKind::Single);
            return true;
        }
        let before = match self.loop_.mode() {
            crate::WorkKind::Goal => crate::WorkKind::Single,
            mode => mode,
        };
        let Some(gates) = self.goal_gates() else {
            return true;
        };
        if let Err(error) = self.loop_.select_goal(objective, &gates) {
            self.fail(&error.to_string());
            return true;
        }
        announce_run_mode(&self.loop_, &self.ui);
        let objective = objective.to_string();
        let keep_running = self.run_work(&crate::Job::Goal { objective, gates }).await;
        self.leave_goal(before);
        keep_running
    }

    fn goal_gates(&mut self) -> Option<Vec<String>> {
        let gates = self.loop_.verify_commands().to_vec();
        if !gates.is_empty() {
            return Some(gates);
        }
        match crate::verify::check_commands(self.loop_.workspace()) {
            Ok(Some(_)) => {
                self.system(
                    "check: none; Verify is off, so the judge decides when the goal is met. Turn Verify on to gate it",
                );
                return Some(Vec::new());
            }
            Ok(None) => {}
            Err(error) => {
                self.fail(&error.to_string());
                return None;
            }
        }
        if let Some(check) = crate::language::check(self.loop_.workspace()) {
            self.system(&format!(
                "check: {check} (detected; set check in .summonr/check.toml to change it)"
            ));
            return Some(vec![check.to_string()]);
        }
        self.system(
            "check: none; the judge decides when the goal is met. Set check = \"CMD\" in .summonr/check.toml to gate it",
        );
        Some(Vec::new())
    }

    async fn run_work(&mut self, job: &crate::Job) -> bool {
        let code = loop {
            let prompt = match self.loop_.next_action(job) {
                Ok(crate::NextAction::Done(code)) => break code,
                Ok(crate::NextAction::Run { prompt, .. }) => prompt,
                Err(error) => {
                    self.fail(&error.to_string());
                    return true;
                }
            };
            let (command, notice, paused) = match job {
                crate::Job::Goal { objective, .. } => (
                    crate::SlashCommand::Goal,
                    format!(
                        "goal cleared: {}",
                        objective.lines().next().unwrap_or_default()
                    ),
                    "goal paused: cancelled by user",
                ),
                crate::Job::Steps { .. } => (
                    crate::SlashCommand::Steps,
                    "steps cleared".to_string(),
                    "steps paused: cancelled by user; /steps PLAN.md resumes",
                ),
            };
            let reason = format!("{} cleared by user", job.label());
            if self.cleared(command, &reason, &notice) {
                return true;
            }
            if let Some(keep_running) = self
                .work_turn(prompt, paused, job.aborted(), job.label())
                .await
            {
                return keep_running;
            }
        };
        let outcome = match (job, code) {
            (crate::Job::Goal { .. }, 0) => "goal achieved".to_string(),
            (crate::Job::Steps { steps, .. }, 0) => {
                format!("steps complete: {0} of {0}", steps.len())
            }
            (crate::Job::Goal { .. }, code) => format!(
                "goal stopped (exit {code}): {}",
                self.stop_reason()
                    .unwrap_or_else(|| "resume with `summonr --goal`".to_string())
            ),
            (crate::Job::Steps { .. }, code) => format!(
                "steps stopped (exit {code}): {}",
                self.stop_reason().unwrap_or_default()
            ),
        };
        self.outcome(&outcome);
        true
    }

    fn leave_goal(&mut self, mode: crate::WorkKind) {
        if self.loop_.mode() == crate::WorkKind::Goal
            && self.loop_.active_goal().ok().flatten().is_none()
        {
            self.loop_.set_mode(mode);
            drop(self.ui.send(Msg::RunMode(mode)));
        }
    }

    fn clear_goal(&mut self) {
        match self.loop_.active_goal().ok().flatten() {
            Some((objective, _)) => {
                if let Err(error) = self.loop_.abandon_work("goal cleared by user") {
                    self.fail(&error.to_string());
                } else {
                    self.system(&format!("goal cleared: {objective}"));
                }
            }
            None => self.system("no active goal"),
        }
    }

    fn abandon(&mut self, reason: &str) {
        if let Err(error) = self.loop_.abandon_work(reason) {
            self.fail(&error.to_string());
        }
    }

    fn cleared(&mut self, command: crate::SlashCommand, reason: &str, notice: &str) -> bool {
        if !self.take_clear(command) {
            return false;
        }
        self.abandon(reason);
        self.system(notice);
        true
    }

    async fn work_turn(
        &mut self,
        prompt: String,
        paused: &str,
        aborted: &str,
        label: &str,
    ) -> Option<bool> {
        if !self.turn(prompt.into()).await {
            return Some(false);
        }
        match self.last_turn.take() {
            None => {
                self.system(paused);
                Some(true)
            }
            Some(turn) if turn.aborted => {
                if !self.stop_work(aborted).await {
                    return Some(false);
                }
                self.system(&format!("{label} stopped; the turn aborted: {}", turn.text));
                Some(true)
            }
            Some(_) => None,
        }
    }

    async fn stop_work(&mut self, reason: &str) -> bool {
        let ui = self.ui.clone();
        let outcome = wait_cancelable(
            &mut self.cmd,
            &mut self.queue,
            &self.reader,
            &self.ui,
            None,
            self.loop_.stop_work(reason, move |event| {
                drop(ui.send(Msg::Event(event.clone())));
            }),
        )
        .await;
        match outcome {
            Completion::Done(Ok(_)) => true,
            Completion::Done(Err(error)) => {
                self.fail(&error.to_string());
                true
            }
            Completion::Cancelled => {
                self.abandon(reason);
                true
            }
            Completion::Quit => false,
        }
    }

    fn stop_reason(&self) -> Option<String> {
        self.loop_
            .work_progress()
            .ok()
            .and_then(|progress| progress.reason)
    }

    pub(in crate::tui::worker) fn goal_status(&mut self) -> bool {
        if let Some(status) = self.loop_.goal_status() {
            self.system(&status);
        } else {
            self.system(
                "/goal <objective>: work toward it across turns; gates come from .summonr/check.toml; /goal clear ends it",
            );
        }
        true
    }
}

pub(super) fn is_stop_word(argument: &str) -> bool {
    matches!(argument, "clear" | "stop" | "off" | "cancel")
}
