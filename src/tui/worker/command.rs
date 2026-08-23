use super::{Msg, Worker, set_mode};
use crate::goal::Engine as GoalEngine;
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
        set_mode(&mut self.loop_, &self.ui, false);
        self.system("plan kept in the session journal — ask to implement it");
        true
    }

    fn reject_plan(&mut self) -> bool {
        self.system("plan rejected — session keeps the journaled revision");
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

    /// `/goal <objective>`: run the shared goal engine inline. Each
    /// iteration is an ordinary cancellable turn; Esc stops the loop.
    async fn goal(&mut self, objective: &str) -> bool {
        let gates = self.loop_.verify_commands().to_vec();
        let engine = GoalEngine::open(
            self.loop_.workspace(),
            objective,
            self.loop_.session_id().as_str(),
            gates.clone(),
            crate::goal::DEFAULT_MAX_TURNS,
            self.loop_.budget(),
        );
        let mut engine = match engine {
            Ok(engine) => engine,
            Err(error) => {
                self.fail(&error.to_string());
                return true;
            }
        };
        self.loop_.set_verify(None);
        let mut cancelled = false;
        let mut failed = false;
        while engine.exit_code().is_none() {
            let prompt = match engine.prompt() {
                Ok(Some(prompt)) => prompt,
                Ok(None) => break,
                Err(error) => {
                    self.fail(&error.to_string());
                    failed = true;
                    break;
                }
            };
            for note in engine.take_notes() {
                self.system(&note);
            }
            if !self.turn(prompt.into()).await {
                cancelled = true;
                break;
            }
            let Some(turn) = self.last_turn.take() else {
                self.system("goal paused — cancelled by user");
                break;
            };
            if let Err(error) = engine.after_turn(&mut self.loop_, &turn).await {
                self.fail(&error.to_string());
                failed = true;
                break;
            }
            for note in engine.take_notes() {
                self.system(&note);
            }
        }
        self.loop_.set_verify((!gates.is_empty()).then_some(gates));
        if cancelled {
            return false;
        }
        if failed {
            return true;
        }
        match engine.exit_code() {
            Some(0) => self.system("goal achieved"),
            Some(code) => self.system(&format!(
                "goal stopped (exit {code}) — resume with `summon --goal`"
            )),
            None => {}
        }
        true
    }
}
