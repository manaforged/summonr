use super::*;

impl Worker {
    pub(in crate::tui::worker) async fn accept_plan(&mut self) -> bool {
        if let Err(error) = self.loop_.accept_plan() {
            self.fail(&error.to_string());
            return true;
        }
        set_run_mode(&mut self.loop_, &self.ui, crate::RunMode::Work);
        if self.loop_.mode() != crate::RunMode::Work {
            return true;
        }
        self.turn("Execute the accepted plan.".into()).await
    }

    pub(in crate::tui::worker) fn reject_plan(&mut self) -> bool {
        if let Err(error) = self.loop_.reject_plan() {
            self.fail(&error.to_string());
            return true;
        }
        set_run_mode(&mut self.loop_, &self.ui, crate::RunMode::Auto);
        self.system("plan rejected");
        true
    }

    pub(in crate::tui::worker) async fn compact(&mut self) -> bool {
        self.system("Compacting context. Press Escape to cancel.");
        drop(self.ui.send(Msg::Busy(Some(Activity::Compact))));
        let ui = self.ui.clone();
        let outcome = attend(
            Inbox {
                cmd: &mut self.cmd,
                reader: &self.reader,
                followups: &mut self.followups,
                deferred: &mut self.deferred,
                ui: &self.ui,
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
            Attended::Done(Ok(true)) => self.system("context compacted — earlier turns summarized"),
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
                "/steps PLAN.md — run each numbered item under `## Steps`; /steps clear ends it",
            );
            return true;
        }
        if matches!(plan_path, "clear" | "stop" | "off" | "cancel") {
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
            return true;
        }
        let path = self.loop_.workspace().join(plan_path);
        let markdown = match std::fs::read_to_string(&path) {
            Ok(markdown) => markdown,
            Err(error) => {
                self.fail(&format!("cannot read {}: {error}", path.display()));
                return true;
            }
        };
        let steps = match crate::steps::parse(&markdown) {
            Ok(steps) => steps,
            Err(error) => {
                self.fail(&error.to_string());
                return true;
            }
        };
        let total = steps.len();
        let checks = self.loop_.verify_commands().to_vec();
        if let Err(error) = self.loop_.select_steps(&markdown, &checks) {
            self.fail(&error.to_string());
            return true;
        }
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
            let Some(step) = progress
                .current
                .and_then(|current| steps.get(current.saturating_sub(1)))
            else {
                self.fail("Steps Work is outside the selected plan");
                return true;
            };
            if self.take_clear("/steps") {
                if let Err(error) = self.loop_.abandon_work("steps cleared by user") {
                    self.fail(&error.to_string());
                }
                self.system("steps cleared");
                return true;
            }
            let prompt = crate::steps::prompt(step, total, plan_path);
            if !self.turn(prompt.into()).await {
                return false;
            }
            match self.last_turn.take() {
                None => {
                    self.system("steps paused — cancelled by user; /steps PLAN.md resumes");
                    return true;
                }
                Some(turn) if turn.aborted => {
                    if let Err(error) = self.loop_.abandon_work("step execution aborted") {
                        self.fail(&error.to_string());
                    }
                    self.system("steps stopped — a turn hit the round limit");
                    return true;
                }
                Some(_) => {}
            }
        };
        match code {
            0 => self.system(&format!("steps complete — {total} of {total}")),
            code => {
                let reason = self
                    .loop_
                    .work_progress()
                    .ok()
                    .and_then(|progress| progress.reason)
                    .unwrap_or_default();
                self.system(&format!("steps stopped (exit {code}) — {reason}"));
            }
        }
        true
    }

    pub(in crate::tui::worker) async fn goal(&mut self, objective: &str) -> bool {
        let gates = self.loop_.verify_commands().to_vec();
        if objective.is_empty() {
            return self.goal_status();
        }
        if matches!(objective, "clear" | "stop" | "off" | "cancel") {
            match self.loop_.active_goal().ok().flatten() {
                Some((objective, _)) => {
                    if let Err(error) = self.loop_.abandon_work("goal cleared by user") {
                        self.fail(&error.to_string());
                    } else {
                        self.system(&format!("goal cleared — {objective}"));
                    }
                }
                None => self.system("no active goal"),
            }
            return true;
        }
        if let Err(error) = self.loop_.select_goal(objective, &gates, 0) {
            self.fail(&error.to_string());
            return true;
        }
        set_run_mode(&mut self.loop_, &self.ui, crate::RunMode::Goal);
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
            if self.take_clear("/goal") {
                if let Err(error) = self.loop_.abandon_work("goal cleared by user") {
                    self.fail(&error.to_string());
                }
                self.system(&format!("goal cleared — {objective}"));
                return true;
            }
            let prompt = crate::goal::prompt(objective, &gates, progress.rounds);
            if !self.turn(prompt.into()).await {
                return false;
            }
            match self.last_turn.take() {
                None => {
                    self.system("goal paused — cancelled by user");
                    return true;
                }
                Some(turn) if turn.aborted => {
                    if let Err(error) = self.loop_.abandon_work("goal execution aborted") {
                        self.fail(&error.to_string());
                    }
                    self.system("goal stopped — a turn hit the round limit");
                    return true;
                }
                Some(_) => {}
            }
        };
        match code {
            0 => self.system("goal achieved"),
            code => {
                let reason = self
                    .loop_
                    .work_progress()
                    .ok()
                    .and_then(|progress| progress.reason)
                    .unwrap_or_else(|| "resume with `summonr --goal`".to_string());
                self.system(&format!("goal stopped (exit {code}) — {reason}"));
            }
        }
        true
    }

    pub(in crate::tui::worker) fn goal_status(&mut self) -> bool {
        if let Some(status) = self.loop_.goal_status() {
            self.system(&status);
        } else {
            self.system(
                "/goal <objective> — work toward it across turns; gates come from .mage/check.toml; /goal clear ends it",
            );
        }
        true
    }
}
