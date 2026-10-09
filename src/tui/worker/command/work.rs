use super::*;

impl Worker {
    pub(in crate::tui::worker) async fn compact(&mut self) -> bool {
        drop(self.ui.send(Msg::Busy(Some(Activity::Compact))));
        let ui = self.ui.clone();
        let outcome = attend(
            Inbox {
                cmd: &mut self.cmd,
                reader: &self.reader,
                followups: &mut self.followups,
                deferred: &mut self.deferred,
                ui: &self.ui,
                answers: None,
            },
            self.loop_
                .compact_with(move |event| drop(ui.send(Msg::Event(event.clone())))),
        )
        .await;
        if !matches!(outcome, Attended::Done(Ok(_))) {
            self.recover();
        }
        drop(self.ui.send(Msg::Busy(None)));
        match outcome {
            Attended::Done(Ok(true)) => self.system("context compacted: earlier turns summarized"),
            Attended::Done(Ok(false)) => self.system("nothing to compact yet"),
            Attended::Done(Err(error)) => self.fail(&error.to_string()),
            Attended::Cancelled => {
                self.system("Compaction cancelled. The conversation is unchanged.");
            }
            Attended::Quit => return false,
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
        if clears(plan_path) {
            self.clear_steps();
            return true;
        }
        let Some((markdown, steps)) = self.read_steps(plan_path) else {
            return true;
        };
        let total = steps.len();
        let checks = self.loop_.verify_commands().to_vec();
        if let Err(error) = self.loop_.select_steps(&markdown, &checks) {
            self.fail(&error.to_string());
            return true;
        }
        announce_run_mode(&self.loop_, &self.ui);
        let code = loop {
            let prompt = match self.loop_.next_step(&steps, plan_path) {
                Ok(crate::StepRun::Done(code)) => break code,
                Ok(crate::StepRun::Run { prompt, .. }) => prompt,
                Err(error) => {
                    self.fail(&error.to_string());
                    return true;
                }
            };
            if self.cleared("/steps", "steps cleared by user", "steps cleared") {
                return true;
            }
            let paused = "steps paused: cancelled by user; /steps PLAN.md resumes";
            if let Some(keep_running) = self
                .work_turn(prompt, paused, "step execution aborted", "steps")
                .await
            {
                return keep_running;
            }
        };
        if code == 0 {
            self.system(&format!("steps complete: {total} of {total}"));
        } else {
            let reason = self.stop_reason().unwrap_or_default();
            self.system(&format!("steps stopped (exit {code}): {reason}"));
        }
        true
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

    fn read_steps(&mut self, plan_path: &str) -> Option<(String, Vec<crate::steps::Step>)> {
        let path = self.loop_.workspace().join(plan_path);
        let markdown = match std::fs::read_to_string(&path) {
            Ok(markdown) => markdown,
            Err(error) => {
                self.fail(&format!("cannot read {}: {error}", path.display()));
                return None;
            }
        };
        match crate::steps::parse(&markdown) {
            Ok(steps) => Some((markdown, steps)),
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
        if clears(objective) {
            self.clear_goal();
            self.leave_goal(crate::RunMode::Auto);
            return true;
        }
        let before = match self.loop_.mode() {
            crate::RunMode::Goal => crate::RunMode::Auto,
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
        let keep_running = self.run_goal(objective, &gates).await;
        self.leave_goal(before);
        keep_running
    }

    fn goal_gates(&mut self) -> Option<Vec<String>> {
        let gates = self.loop_.verify_commands().to_vec();
        if !gates.is_empty() {
            return Some(gates);
        }
        match crate::verify::check_commands(self.loop_.workspace()) {
            Ok(Some(commands)) => return Some(commands),
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

    async fn run_goal(&mut self, objective: &str, gates: &[String]) -> bool {
        let code = loop {
            let progress = match self.loop_.work_progress() {
                Ok(progress) => progress,
                Err(error) => {
                    self.fail(&error.to_string());
                    return true;
                }
            };
            if let Some(code) = progress.exit_code() {
                break code;
            }
            let title = objective.lines().next().unwrap_or_default();
            let notice = format!("goal cleared: {title}");
            if self.cleared("/goal", "goal cleared by user", &notice) {
                return true;
            }
            let prompt = crate::goal::prompt(objective, gates, progress.rounds);
            if let Some(keep_running) = self
                .work_turn(
                    prompt,
                    "goal paused: cancelled by user",
                    "goal execution aborted",
                    "goal",
                )
                .await
            {
                return keep_running;
            }
        };
        if code == 0 {
            self.system("goal achieved");
        } else {
            let reason = self
                .stop_reason()
                .unwrap_or_else(|| "resume with `summonr --goal`".to_string());
            self.system(&format!("goal stopped (exit {code}): {reason}"));
        }
        true
    }

    fn leave_goal(&mut self, mode: crate::RunMode) {
        if self.loop_.mode() == crate::RunMode::Goal
            && self.loop_.active_goal().ok().flatten().is_none()
        {
            set_run_mode(&mut self.loop_, &self.ui, mode);
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

    fn cleared(&mut self, command: &str, reason: &str, notice: &str) -> bool {
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
                self.abandon(aborted);
                self.system(&format!("{label} stopped; the turn aborted: {}", turn.text));
                Some(true)
            }
            Some(_) => None,
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

fn clears(argument: &str) -> bool {
    matches!(argument, "clear" | "stop" | "off" | "cancel")
}
