use super::shell::{shell_prefix, shell_prompt};
use super::*;

impl Worker {
    pub(in crate::tui::worker) async fn submit(&mut self, input: UserInput) -> bool {
        if input.images.is_empty() {
            let line = input.text.trim();
            if let Some((send, command)) = shell_prefix(line) {
                return self.shell(command, send).await;
            }
            if let Some(keep_running) = self.command(line).await {
                return keep_running;
            }
        }
        self.turn(input).await
    }

    pub(in crate::tui::worker) async fn shell(&mut self, command: &str, send: bool) -> bool {
        drop(self.ui.send(Msg::Busy(Some(Activity::Shell))));
        let id = format!("shell-{}", self.loop_.session_id());
        drop(self.ui.send(Msg::Event(crate::tools::Event::ToolCall {
            id: id.clone(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({ "command": command }).to_string(),
        })));
        let started = std::time::Instant::now();
        let (output, success) = match self.loop_.shell(command).await {
            Ok(result) => result,
            Err(error) => (error.to_string(), false),
        };
        drop(self.ui.send(Msg::Event(crate::tools::Event::ToolResult {
            id,
            success,
            mutated: false,
            output: output.clone(),
            millis: u64::try_from(started.elapsed().as_millis()).ok(),
        })));
        drop(self.ui.send(Msg::Busy(None)));
        if send {
            self.turn(UserInput::from(shell_prompt(command, &output)))
                .await
        } else {
            true
        }
    }

    pub(in crate::tui::worker) async fn command(&mut self, line: &str) -> Option<bool> {
        if line.is_empty() {
            return Some(true);
        }
        let argument = match argument(line) {
            Ok(argument) => argument,
            Err(error) => {
                self.fail(&error);
                return Some(true);
            }
        };
        let handled = match crate::parse_command(line) {
            Some(crate::SlashCommand::Exit) => {
                drop(self.ui.send(Msg::Bye));
                return Some(false);
            }
            Some(crate::SlashCommand::Help) => {
                drop(self.ui.send(Msg::OpenHelp));
                true
            }
            Some(crate::SlashCommand::Auth | crate::SlashCommand::Login) => {
                let name = argument.unwrap_or(&self.endpoint).to_string();
                return Some(self.login(&name).await);
            }
            Some(crate::SlashCommand::Workflow) => {
                self.workflow(argument);
                true
            }
            Some(crate::SlashCommand::Clear) => self.clear(),
            Some(crate::SlashCommand::Compact) => self.compact().await,
            Some(crate::SlashCommand::PlanAccept) => return Some(self.accept_plan().await),
            Some(crate::SlashCommand::PlanReject) => self.reject_plan(),
            Some(crate::SlashCommand::Plan) => {
                set_run_mode(&mut self.loop_, &self.ui, crate::RunMode::Plan);
                true
            }
            Some(crate::SlashCommand::Auto) => {
                set_run_mode(&mut self.loop_, &self.ui, crate::RunMode::Auto);
                true
            }
            Some(crate::SlashCommand::Goal) => {
                return Some(self.goal(argument.unwrap_or_default()).await);
            }
            Some(crate::SlashCommand::Session) => self.session(),
            Some(crate::SlashCommand::Profile) => {
                match crate::profile_session(self.loop_.session_path()) {
                    Ok(report) => self.system(report.trim_end()),
                    Err(error) => self.fail(&error.to_string()),
                }
                true
            }
            Some(crate::SlashCommand::Resume) => self.resume(argument),
            Some(crate::SlashCommand::Retry) => return Some(self.retry().await),
            Some(crate::SlashCommand::Name) => self.name(argument),
            Some(crate::SlashCommand::Steps) => {
                return Some(self.steps(argument.unwrap_or_default()).await);
            }
            Some(crate::SlashCommand::Effort) => self.effort(argument),
            Some(crate::SlashCommand::Network) => self.network(argument),
            Some(crate::SlashCommand::Write) => self.write(argument),
            Some(crate::SlashCommand::Approve) => self.approve(argument),
            Some(crate::SlashCommand::Fork) => self.fork(),
            Some(crate::SlashCommand::Rollback) => self.rollback(argument),
            Some(crate::SlashCommand::Undo) => self.undo(),
            Some(crate::SlashCommand::Agents) => self.agents(),
            Some(crate::SlashCommand::Modules) => self.modules(argument),
            Some(_) => self.unknown(line).unwrap_or(true),
            None if line.starts_with('/') => self.unknown(line).unwrap_or(true),
            _ => false,
        };
        handled.then_some(true)
    }

    pub(in crate::tui::worker) fn clear(&mut self) -> bool {
        match self.loop_.clear_history() {
            Ok(()) => drop(self.ui.send(Msg::Cleared)),
            Err(error) => self.fail(&error.to_string()),
        }
        true
    }

    pub(in crate::tui::worker) fn write(&mut self, argument: Option<&str>) -> bool {
        match argument.map(str::trim) {
            None | Some("") => {
                let roots = self.loop_.write_roots();
                let list = if roots.is_empty() {
                    "no extra write paths — /write PATH persists to [sandbox] write".to_string()
                } else {
                    roots.join(" · ")
                };
                self.system(&format!("write paths: {list}"));
            }
            Some(path) => {
                let (persist, path) = match path.strip_prefix("--session ") {
                    Some(rest) => (false, rest.trim()),
                    None => (true, path),
                };
                if path.is_empty() {
                    self.system("/write PATH  (or /write --session PATH)");
                    return true;
                }
                let protected = crate::policy::protected_write(&crate::policy::expand_path(path));
                match self.loop_.allow_path(path) {
                    Ok(()) => {
                        if persist && !protected {
                            match crate::config::path() {
                                Ok(config) => {
                                    match crate::config::save_sandbox_write(
                                        &config,
                                        &crate::config::sandbox_entry(
                                            &crate::policy::expand_path(path),
                                        ),
                                    ) {
                                        Ok(()) => self.system(&format!(
                                            "writes allowed under {path} and saved to [sandbox] write"
                                        )),
                                        Err(error) => self.system(&format!(
                                            "writes allowed under {path} for this session; standing grant not saved: {error}"
                                        )),
                                    }
                                }
                                Err(error) => self.system(&format!(
                                    "writes allowed under {path} for this session; standing grant not saved: {error}"
                                )),
                            }
                        } else if protected {
                            self.system(&format!(
                                "writes allowed under {path} for this session; a protected path is never saved to [sandbox] write"
                            ));
                        } else {
                            self.system(&format!("writes allowed under {path} for this session"));
                        }
                    }
                    Err(error) => self.fail(&error.to_string()),
                }
            }
        }
        true
    }

    pub(in crate::tui::worker) fn approve(&mut self, argument: Option<&str>) -> bool {
        match argument.map(str::trim) {
            Some("on") => {
                self.loop_.set_manual(true);
                self.system("manual approval on — extra-root writes and shell commands ask first");
            }
            Some("off") => {
                self.loop_.set_manual(false);
                self.system(
                    "manual approval off — auto mode grants extra-root writes for this session",
                );
            }
            None | Some("") => {
                let state = if self.loop_.manual() { "on" } else { "off" };
                self.system(&format!("manual approval {state} — /approve on|off"));
            }
            Some(_) => self.system("/approve on | off"),
        }
        true
    }

    pub(in crate::tui::worker) async fn retry(&mut self) -> bool {
        let lines = match log::read(self.loop_.session_path()) {
            Ok(lines) => lines,
            Err(error) => {
                self.fail(&format!("Cannot read the last turn: {error}"));
                return true;
            }
        };
        let interrupted = lines.iter().rev().find_map(|line| match line {
            Line::Close { reason } => Some(reason.is_some()),
            Line::Clear | Line::User { start: true, .. } => Some(false),
            _ => None,
        });
        if interrupted != Some(true) {
            self.system("No failed or cancelled turn to retry.");
            return true;
        }
        self.system("Continuing the interrupted turn from its saved results.");
        self.turn(
            "Continue the interrupted request from the current conversation and workspace. Use recorded tool results to avoid repeating completed actions."
                .into(),
        )
        .await
    }

    pub(in crate::tui::worker) fn modules(&mut self, argument: Option<&str>) -> bool {
        let workspace = self.loop_.workspace().to_path_buf();
        if let Some(argument) = argument {
            let mut words = argument.split_whitespace();
            let (Some(name), Some(state)) = (words.next(), words.next()) else {
                self.system("/modules NAME on|off");
                return true;
            };
            let on = match state {
                "on" => true,
                "off" => false,
                _ => {
                    self.system("/modules NAME on|off");
                    return true;
                }
            };
            if let Err(error) = crate::modules::set(name, on) {
                self.fail(&error.to_string());
                return true;
            }
        }
        match crate::modules::Switches::load(&workspace) {
            Ok(switches) => {
                if argument.is_some() {
                    if self.loop_.catalog_frozen() {
                        self.system("saved for the next process; this session's tools stay frozen");
                    } else {
                        self.loop_.set_modules(&switches);
                    }
                }
                self.system(&switches.table());
            }
            Err(error) => self.fail(&error.to_string()),
        }
        true
    }

    pub(in crate::tui::worker) fn agents(&self) -> bool {
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

    pub(in crate::tui::worker) fn unknown(&self, line: &str) -> Option<bool> {
        let head = line.split_whitespace().next().unwrap_or(line);
        self.fail(&format!(
            "unknown command `{head}` — /help lists them, tab completes"
        ));
        Some(true)
    }
}
