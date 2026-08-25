use super::{Msg, Worker, set_mode};
use crate::provider::UserInput;

impl Worker {
    pub(super) async fn submit(&mut self, input: UserInput) -> bool {
        if input.images.is_empty() {
            let line = input.text.trim();
            if let Some(keep_running) = self.command(line).await {
                return keep_running;
            }
            if let Some(shell) = line.strip_prefix('!') {
                let output = self
                    .loop_
                    .shell(shell)
                    .await
                    .unwrap_or_else(|error| format!("summon: {error}"));
                self.system(&output);
                return true;
            }
        }
        self.turn(input).await
    }

    async fn command(&mut self, line: &str) -> Option<bool> {
        if line.is_empty() {
            return Some(true);
        }
        let handled = match crate::parse_command(line) {
            Some(crate::SlashCommand::Exit) => {
                drop(self.ui.send(Msg::Bye));
                return Some(false);
            }
            Some(crate::SlashCommand::Help) => {
                self.system(&super::super::help_text());
                true
            }
            Some(crate::SlashCommand::Clear) => self.clear(),
            Some(crate::SlashCommand::PlanAccept) => self.accept_plan(),
            Some(crate::SlashCommand::PlanReject) => self.reject_plan(),
            Some(crate::SlashCommand::Plan) => {
                let plan = !self.loop_.is_plan();
                set_mode(&mut self.loop_, &self.ui, plan);
                true
            }
            Some(crate::SlashCommand::Auto) => {
                set_mode(&mut self.loop_, &self.ui, false);
                true
            }
            Some(crate::SlashCommand::Goal) => {
                let text = line.trim_start_matches("/goal").trim().to_string();
                if text.is_empty() {
                    self.system("/goal <objective> — work toward it across turns; gates come from .mage/check.toml");
                    true
                } else {
                    return Some(self.goal(&text).await);
                }
            }
            Some(crate::SlashCommand::Session) => self.session(),
            Some(crate::SlashCommand::Fork) => self.fork(),
            Some(crate::SlashCommand::Agents) => self.agents(),
            Some(_) => self.unknown(line).unwrap_or(true),
            None if line.starts_with('/') => self.unknown(line).unwrap_or(true),
            _ => false,
        };
        handled.then_some(true)
    }

    fn clear(&mut self) -> bool {
        match self.loop_.clear_history() {
            Ok(()) => drop(self.ui.send(Msg::Cleared)),
            Err(error) => self.fail(&error.to_string()),
        }
        true
    }

    fn accept_plan(&mut self) -> bool {
        if let Err(error) = self.loop_.accept_plan() {
            self.fail(&error.to_string());
            return true;
        }
        set_mode(&mut self.loop_, &self.ui, false);
        self.system("Plan accepted. Your next prompt executes in that Work.");
        true
    }

    fn reject_plan(&mut self) -> bool {
        if let Err(error) = self.loop_.abandon_work("plan rejected by the user") {
            self.fail(&error.to_string());
            return true;
        }
        set_mode(&mut self.loop_, &self.ui, false);
        self.system("plan rejected");
        true
    }

    fn session(&self) -> bool {
        self.system(&format!(
            "{} · {}",
            self.loop_.session_id(),
            self.loop_.session_path().display()
        ));
        true
    }

    fn fork(&mut self) -> bool {
        match self.loop_.fork_live() {
            Ok(id) => self.system(&format!(
                "forked to session {id} — this branch continues here"
            )),
            Err(error) => self.fail(&error.to_string()),
        }
        true
    }

    fn agents(&self) -> bool {
        let roster = self.loop_.bus_agents();
        let text = if roster.is_empty() {
            "no live sessions on the bus".to_string()
        } else {
            roster
                .iter()
                .map(|agent| {
                    let me = if agent.id == self.loop_.session_id() {
                        " (you)"
                    } else {
                        ""
                    };
                    format!(
                        "{} · {} · {} · {}{me}",
                        agent.id, agent.model, agent.cwd, agent.status
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        self.system(&text);
        true
    }

    fn unknown(&self, line: &str) -> Option<bool> {
        let head = line.split_whitespace().next().unwrap_or(line);
        self.fail(&format!(
            "unknown command `{head}` — /help lists them, tab completes"
        ));
        Some(true)
    }

    /// `/goal <objective>`: run turns until Work records a terminal verdict.
    async fn goal(&mut self, objective: &str) -> bool {
        let gates = self.loop_.verify_commands().to_vec();
        if let Err(error) = self
            .loop_
            .select_goal(objective, &gates, crate::DEFAULT_MAX_TURNS)
        {
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
            let prompt = crate::goal::prompt(objective, &gates, progress.rounds);
            if !self.turn(prompt.into()).await {
                return false;
            }
            if self.last_turn.take().is_none() {
                self.system("goal paused — cancelled by user");
                return true;
            }
        };
        match code {
            0 => self.system("goal achieved"),
            code => self.system(&format!(
                "goal stopped (exit {code}) — resume with `summon --goal`"
            )),
        }
        true
    }
}
