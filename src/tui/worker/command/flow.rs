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
        let mut input = input;
        if let Some(notes) = crate::hunks::drain(self.loop_.workspace()) {
            input.text = format!("{notes}\n\n{}", input.text);
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
            patch: None,
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
        match crate::parse_command(line) {
            Some(command) => self.slash(command, line, argument).await,
            None if line.starts_with('/') => Some(self.unknown(line).unwrap_or(true)),
            None => None,
        }
    }

    async fn slash(
        &mut self,
        command: crate::SlashCommand,
        line: &str,
        argument: Option<&str>,
    ) -> Option<bool> {
        match command {
            crate::SlashCommand::Exit => {
                drop(self.ui.send(Msg::Bye));
                Some(false)
            }
            crate::SlashCommand::Login => {
                let name = argument.unwrap_or(&self.endpoint).to_string();
                Some(self.login(&name).await)
            }
            crate::SlashCommand::Compact => self.compact().await.then_some(true),
            crate::SlashCommand::PlanAccept => Some(self.accept_plan().await),
            crate::SlashCommand::Goal => Some(self.goal(argument.unwrap_or_default()).await),
            crate::SlashCommand::Retry => Some(self.retry().await),
            crate::SlashCommand::Steps => Some(self.steps(argument.unwrap_or_default()).await),
            crate::SlashCommand::Plan => {
                set_run_mode(&mut self.loop_, &self.ui, crate::RunMode::Plan);
                Some(true)
            }
            crate::SlashCommand::Auto => {
                set_run_mode(&mut self.loop_, &self.ui, crate::RunMode::Auto);
                Some(true)
            }
            other => self.local(other, line, argument).then_some(true),
        }
    }

    fn local(&mut self, command: crate::SlashCommand, line: &str, argument: Option<&str>) -> bool {
        match command {
            crate::SlashCommand::Help => {
                drop(self.ui.send(Msg::OpenHelp));
                true
            }
            crate::SlashCommand::Workflow => {
                self.workflow(argument);
                true
            }
            crate::SlashCommand::Clear => self.clear(),
            crate::SlashCommand::PlanReject => self.reject_plan(),
            crate::SlashCommand::Status => self.session(),
            crate::SlashCommand::Usage => self.usage(argument),
            crate::SlashCommand::Resume => self.resume(argument),
            crate::SlashCommand::Name => self.name(argument),
            crate::SlashCommand::Permissions => self.permissions(argument),
            crate::SlashCommand::Fork => self.fork(),
            crate::SlashCommand::Undo => self.undo_command(argument),
            crate::SlashCommand::Agents => self.agents(),
            _ => self.unknown(line).unwrap_or(true),
        }
    }

    fn usage(&mut self, argument: Option<&str>) -> bool {
        match argument {
            Some("session") => {
                match crate::profile(crate::ProfileScope::Session(self.loop_.session_path())) {
                    Ok(report) => self.system(report.trim_end()),
                    Err(error) => self.fail(&error.to_string()),
                }
            }
            Some(other) => self.fail(&format!("/usage takes `session`, not `{other}`")),
            None => {}
        }
        true
    }

    fn undo_command(&mut self, argument: Option<&str>) -> bool {
        match argument.map(|text| text.strip_prefix("files")) {
            None => self.undo(),
            Some(Some(turn)) => self.rollback(Some(turn)),
            Some(None) => {
                self.fail("/undo takes `files` or `files N`");
                true
            }
        }
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
                    "no extra write paths; /permissions write PATH persists to [sandbox] write"
                        .to_string()
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
                    self.system("/permissions write [--session] PATH");
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

    pub(in crate::tui::worker) async fn retry(&mut self) -> bool {
        let lines = match log::read(self.loop_.session_path()) {
            Ok(lines) => lines,
            Err(error) => {
                self.fail(&format!("Cannot read the last turn: {error}"));
                return true;
            }
        };
        let interrupted = lines.iter().rev().find_map(|line| match line {
            Line::Close { reason, .. } => Some(reason.is_some()),
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

    pub(in crate::tui::worker) fn agents(&self) -> bool {
        let text = crate::mail::roster(&self.loop_.peers(), &self.loop_.session_id());
        self.system(&text);
        true
    }

    pub(in crate::tui::worker) fn unknown(&self, line: &str) -> Option<bool> {
        let head = line.split_whitespace().next().unwrap_or(line);
        self.fail(&format!(
            "unknown command `{head}`; /help lists them, tab completes"
        ));
        Some(true)
    }
}
