use std::collections::VecDeque;

use tokio::sync::mpsc::UnboundedReceiver;

use super::reader::Reader;
use super::{Cmd, Msg, Worker, set_mode};
use crate::command::argument;
use crate::durable::log::{self, Line};
use crate::provider::UserInput;
use crate::tui::Activity;

struct Inbox<'a> {
    cmd: &'a mut UnboundedReceiver<Cmd>,
    reader: &'a Reader,
    followups: &'a mut VecDeque<UserInput>,
    deferred: &'a mut VecDeque<Cmd>,
    ui: &'a tokio::sync::mpsc::UnboundedSender<Msg>,
}

enum Attended<T> {
    Done(T),
    Cancelled,
    Quit,
}

async fn attend<T>(inbox: Inbox<'_>, task: impl Future<Output = T>) -> Attended<T> {
    tokio::pin!(task);
    loop {
        tokio::select! {
            result = &mut task => break Attended::Done(result),
            command = inbox.cmd.recv() => match command {
                Some(Cmd::Cancel) => break Attended::Cancelled,
                Some(Cmd::Quit) | None => break Attended::Quit,
                Some(Cmd::Tick) => {}
                Some(command @ Cmd::ListPrompts(_)) => inbox.reader.run(command).await,
                Some(Cmd::Submit(input) | Cmd::Steer(input) | Cmd::Queue(input)) => {
                    inbox.followups.push_back(input);
                    drop(inbox.ui.send(Msg::Queued {
                        steering: 0,
                        queued: inbox.followups.len(),
                    }));
                }
                Some(command) => inbox.deferred.push_back(command),
            }
        }
    }
}

impl Worker {
    pub(super) async fn submit(&mut self, input: UserInput) -> bool {
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

    async fn shell(&mut self, command: &str, send: bool) -> bool {
        drop(self.ui.send(Msg::Busy(Some(Activity::Shell))));
        let id = format!("shell-{}", self.loop_.session_id());
        drop(self.ui.send(Msg::Event(crate::tools::Event::ToolCall {
            id: id.clone(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({ "command": command }).to_string(),
        })));
        let (output, success) = match self.loop_.shell(command).await {
            Ok(result) => result,
            Err(error) => (error.to_string(), false),
        };
        drop(self.ui.send(Msg::Event(crate::tools::Event::ToolResult {
            id,
            success,
            mutated: false,
            output: output.clone(),
        })));
        drop(self.ui.send(Msg::Busy(None)));
        if send {
            self.turn(UserInput::from(shell_prompt(command, &output)))
                .await
        } else {
            true
        }
    }

    pub(super) async fn command(&mut self, line: &str) -> Option<bool> {
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
            Some(crate::SlashCommand::PlanAccept) => self.accept_plan(),
            Some(crate::SlashCommand::PlanReject) => self.reject_plan(),
            Some(crate::SlashCommand::Plan) => {
                set_mode(&mut self.loop_, &self.ui, true);
                true
            }
            Some(crate::SlashCommand::Auto) => {
                set_mode(&mut self.loop_, &self.ui, false);
                true
            }
            Some(crate::SlashCommand::Goal) => {
                return Some(self.goal(argument.unwrap_or_default()).await);
            }
            Some(crate::SlashCommand::Session) => self.session(),
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
            Some(crate::SlashCommand::Agents) => self.agents(),
            Some(crate::SlashCommand::Modules) => self.modules(argument),
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

    fn network(&mut self, argument: Option<&str>) -> bool {
        match argument.map(str::trim) {
            None | Some("") => {
                let state = if !self.loop_.has_sandbox() {
                    "sandbox off — commands already have network"
                } else if self.loop_.denies_network() {
                    "network denied in the sandbox — /net on for this session or /net once"
                } else {
                    "network allowed for this session — /net off to deny again"
                };
                self.system(state);
            }
            Some("on") => match self.loop_.set_network(true) {
                Ok(()) => self.system("network allowed for this session"),
                Err(error) => self.fail(&error.to_string()),
            },
            Some("off") => match self.loop_.set_network(false) {
                Ok(()) => self.system("network denied for this session"),
                Err(error) => self.fail(&error.to_string()),
            },
            Some("once") => {
                self.loop_.allow_network_once();
                self.system("network allowed for the next command");
            }
            Some(_) => self.system("/net on | off | once"),
        }
        true
    }

    fn write(&mut self, argument: Option<&str>) -> bool {
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
                match self.loop_.allow_path(path) {
                    Ok(()) => {
                        if persist {
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

    fn approve(&mut self, argument: Option<&str>) -> bool {
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

    async fn retry(&mut self) -> bool {
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

    fn accept_plan(&mut self) -> bool {
        if let Err(error) = self.loop_.accept_plan() {
            self.fail(&error.to_string());
            return true;
        }
        set_mode(&mut self.loop_, &self.ui, false);
        self.system("Plan accepted. Send a prompt to start implementing it.");
        true
    }

    fn reject_plan(&mut self) -> bool {
        if let Err(error) = self.loop_.reject_plan() {
            self.fail(&error.to_string());
            return true;
        }
        set_mode(&mut self.loop_, &self.ui, false);
        self.system("plan rejected");
        true
    }

    fn session(&self) -> bool {
        self.system(&format!(
            "{}{} · {}",
            self.loop_.session_id(),
            self.loop_
                .name()
                .map_or_else(String::new, |name| format!(" ({name})")),
            self.loop_.session_path().display()
        ));
        true
    }

    fn resume(&self, key: Option<&str>) -> bool {
        let selected = match key {
            Some(key) => crate::session::resolve_id(key).and_then(|path| {
                crate::session::list()?
                    .into_iter()
                    .find(|info| info.path == path)
                    .ok_or_else(|| crate::Error::Config(format!("session `{key}` disappeared")))
            }),
            None => crate::session::previous_for(
                self.loop_.workspace(),
                &self.loop_.session_id().to_string(),
            )
            .and_then(|info| {
                info.ok_or_else(|| {
                    crate::Error::Config("no previous session for this workspace".to_string())
                })
            }),
        };
        match selected {
            Ok(info) => drop(self.ui.send(Msg::Resume(info.id))),
            Err(error) => self.fail(&error.to_string()),
        }
        true
    }

    fn name(&self, name: Option<&str>) -> bool {
        match name {
            Some(name) => match self.loop_.set_name(name) {
                Ok(()) => self.system(&format!("session named {name}")),
                Err(error) => self.fail(&error.to_string()),
            },
            None => match self.loop_.name() {
                Some(name) => self.system(&format!("session name: {name}")),
                None => self.system("this session has no name — /name TEXT"),
            },
        }
        true
    }

    fn fork(&mut self) -> bool {
        match self.loop_.fork_live() {
            Ok(id) => {
                self.system(&format!(
                    "Forked to session {id}. This branch continues here."
                ));
                drop(self.ui.send(Msg::Session(id)));
            }
            Err(error) => self.fail(&error.to_string()),
        }
        true
    }

    pub(super) fn branch(&mut self, session: &str, turn: usize) -> bool {
        if self.loop_.session_id() != session {
            self.fail("The active session changed. Open /branch again.");
            drop(self.ui.send(Msg::Busy(None)));
            return true;
        }
        match self.loop_.fork_at(Some(turn)) {
            Ok(id) => {
                self.last_turn = None;
                drop(self.ui.send(Msg::Reset));
                self.replay();
                drop(self.ui.send(Msg::Session(id)));
                self.send_settings();
                self.system(&format!(
                    "Branched after turn {}. Workspace files are unchanged. Use /tree to return to the parent.",
                    turn + 1
                ));
            }
            Err(error) => self.fail(&error.to_string()),
        }
        drop(self.ui.send(Msg::Busy(None)));
        true
    }

    fn modules(&mut self, argument: Option<&str>) -> bool {
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
                    self.loop_.set_modules(&switches);
                }
                self.system(&switches.table());
            }
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

    fn auth(&self, name: &str) {
        match crate::endpoint::auth_info(name) {
            Ok(info) => self.system(&info.status()),
            Err(error) => self.fail(&error.to_string()),
        }
    }

    pub(super) async fn login(&mut self, name: &str) -> bool {
        let info = match crate::endpoint::auth_info(name) {
            Ok(info) => info,
            Err(error) => {
                self.fail(&error.to_string());
                return true;
            }
        };
        if !info.device_login {
            self.auth(name);
            return true;
        }
        let endpoint = match crate::endpoint::resolve(Some(name), None) {
            Ok(endpoint) => endpoint,
            Err(error) => {
                self.fail(&error.to_string());
                return true;
            }
        };
        let notice = self.ui.clone();
        let login = crate::provider::Client::login(&endpoint, move |line| {
            drop(notice.send(Msg::System(line.to_string())));
        });
        self.system(&format!("{name}: signing in. Press Escape to cancel."));
        drop(self.ui.send(Msg::Busy(Some(Activity::Login))));
        let outcome = attend(
            Inbox {
                cmd: &mut self.cmd,
                reader: &self.reader,
                followups: &mut self.followups,
                deferred: &mut self.deferred,
                ui: &self.ui,
            },
            login,
        )
        .await;
        let running = match outcome {
            Attended::Done(Ok(())) => {
                self.system(&format!("{name}: login complete"));
                true
            }
            Attended::Done(Err(error)) => {
                self.fail(&format!("{error}. Use /login {name} to try again."));
                self.recover();
                true
            }
            Attended::Cancelled => {
                self.system(&format!(
                    "{name}: sign-in cancelled. Use /login {name} to try again."
                ));
                self.recover();
                true
            }
            Attended::Quit => false,
        };
        drop(self.ui.send(Msg::Busy(None)));
        running
    }

    fn unknown(&self, line: &str) -> Option<bool> {
        let head = line.split_whitespace().next().unwrap_or(line);
        self.fail(&format!(
            "unknown command `{head}` — /help lists them, tab completes"
        ));
        Some(true)
    }

    async fn compact(&mut self) -> bool {
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

    fn take_clear(&mut self, head: &str) -> bool {
        let position = self.deferred.iter().position(|command| {
            matches!(
                command,
                Cmd::Command(line)
                    if line.trim_start().starts_with(head)
                        && matches!(argument(line), Ok(Some("clear" | "stop" | "off" | "cancel")))
            )
        });
        position.is_some_and(|index| self.deferred.remove(index).is_some())
    }

    async fn steps(&mut self, plan_path: &str) -> bool {
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

    async fn goal(&mut self, objective: &str) -> bool {
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

    fn effort(&mut self, level: Option<&str>) -> bool {
        match level {
            None => {}
            Some(level) => match crate::Effort::parse(level) {
                Some(effort) => {
                    self.dial = Some(effort);
                    self.loop_.set_effort(Some(effort));
                }
                None => {
                    self.fail(&format!(
                        "effort takes none, low, medium, or high, not `{level}`"
                    ));
                    return true;
                }
            },
        }
        let current = self
            .loop_
            .effort()
            .map_or("endpoint default", crate::Effort::as_str);
        let scope = if self.dial.is_some() {
            " · not saved"
        } else {
            ""
        };
        self.system(&format!("effort: {current}{scope}"));
        true
    }

    fn goal_status(&mut self) -> bool {
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

pub(super) fn shell_prefix(line: &str) -> Option<(bool, &str)> {
    let rest = line.strip_prefix('!')?;
    let (send, command) = match rest.strip_prefix('!') {
        Some(quiet) => (false, quiet),
        None => (true, rest),
    };
    let command = command.trim();
    (!command.is_empty()).then_some((send, command))
}

pub(super) fn shell_prompt(command: &str, output: &str) -> String {
    format!("$ {command}\n{output}")
}
