use std::sync::PoisonError;

use tokio::sync::mpsc::UnboundedSender;

use super::settings::set_run_mode;
use super::{Cmd, Msg, Worker};
use crate::provider::UserInput;
use crate::tools::Event;
use crate::tui::Activity;

const MODE_LATER: &str = "mode switches when the current turn ends — esc cancels now";
const SETTINGS_LATER: &str = "settings apply when the current turn ends — esc cancels now";

enum Input {
    User(UserInput),
    Mail,
}

impl Worker {
    pub(super) async fn turn(&mut self, input: UserInput) -> bool {
        self.run_turn(Input::User(input)).await
    }

    pub(super) async fn mail(&mut self) -> bool {
        self.run_turn(Input::Mail).await
    }

    async fn run_turn(&mut self, input: Input) -> bool {
        drop(self.ui.send(Msg::Busy(Some(Activity::Turn))));
        let sender = self.ui.clone();
        let emit = move |event: &Event| drop(sender.send(Msg::Event(event.clone())));
        let (cancelled, result, quit) = {
            let session_path = self.loop_.session_path().to_path_buf();
            let session_id = self.loop_.session_id();
            let turn = async {
                match input {
                    Input::User(input) => self.loop_.turn_with(input, emit).await,
                    Input::Mail => self.loop_.turn_mail_with(emit).await,
                }
            };
            tokio::pin!(turn);
            let (mut cancelled, mut quit) = (false, false);
            let result = loop {
                tokio::select! {
                    done = &mut turn => break Some(done),
                    command = self.cmd.recv() => match command {
                        Some(command @ (Cmd::Command(_) | Cmd::Branch { .. })) => {
                            self.deferred.push_back(command);
                            drop(self.ui.send(Msg::System(SETTINGS_LATER.to_string())));
                        }
                        Some(Cmd::Export(target)) => super::export_session(&session_path, &session_id, target, &self.ui),
                        Some(command @ (Cmd::ListModels { .. } | Cmd::Pin(..) | Cmd::SaveDefault(_) | Cmd::ListSessions | Cmd::ListUsage | Cmd::DeleteSession(_) | Cmd::ListPrompts(_) | Cmd::ListFiles | Cmd::Clipboard | Cmd::Diff(_) | Cmd::Patch(..) | Cmd::Trees | Cmd::View(..) | Cmd::Save(..))) => self.reader.run(command).await,
                        Some(command @ (Cmd::Login(_) | Cmd::SetKey(_, _) | Cmd::Logout(_) | Cmd::Merge(_) | Cmd::Discard(_))) => {
                            self.deferred.push_back(command);
                            drop(self.ui.send(Msg::System(SETTINGS_LATER.to_string())));
                        }
                        Some(Cmd::Cancel) => { cancelled = true; break None; }
                        Some(Cmd::Quit) => { drop(self.ui.send(Msg::Bye)); quit = true; break None; }
                        Some(Cmd::SetRunMode(mode)) => {
                            self.deferred.push_back(Cmd::SetRunMode(mode));
                            drop(self.ui.send(Msg::RunMode(mode)));
                            drop(self.ui.send(Msg::System(MODE_LATER.to_string())));
                        }
                        Some(Cmd::SetEffort(effort)) => {
                            self.deferred.push_back(Cmd::SetEffort(effort));
                        }
                        Some(Cmd::SetBus(on)) => {
                            self.auto_reply = on;
                        }
                        Some(Cmd::Steer(text)) => {
                            if let Ok(mut queue) = self.steering.lock() { queue.push_back(text); }
                            super::announce(&self.ui, &self.steering, &self.followups);
                        }
                        Some(Cmd::Queue(text)) => {
                            self.followups.push_back(text);
                            super::announce(&self.ui, &self.steering, &self.followups);
                        }
                        Some(Cmd::Dequeue) => {
                            let mut pending: Vec<UserInput> = self.steering.lock().map_or_else(|_| Vec::new(), |mut queue| queue.drain(..).collect());
                            pending.extend(self.followups.drain(..));
                            if !pending.is_empty() {
                                drop(self.ui.send(Msg::Restored(pending)));
                            }
                            super::announce(&self.ui, &self.steering, &self.followups);
                        }
                        Some(command @ (Cmd::SetVerify(_) | Cmd::SetEndpoint(_) | Cmd::SetModel(_) | Cmd::SetSeat(_, _) | Cmd::SeatRole(..) | Cmd::ClearRole(_) | Cmd::SaveWorkflow(_) | Cmd::ListWorkflows | Cmd::SetModule(_, _))) => {
                            self.deferred.push_back(command);
                            drop(self.ui.send(Msg::System(SETTINGS_LATER.to_string())));
                        }
                        Some(Cmd::Submit(input)) => {
                            self.followups.push_back(input);
                            super::announce(&self.ui, &self.steering, &self.followups);
                        }
                        Some(Cmd::Mail) => {}
                        Some(Cmd::Send(to, text)) => mail(&self.ui, &session_id, &to, &text),
                        Some(Cmd::Tick | Cmd::LoginAnswer(_)) => {}
                        None => { quit = true; break None; }
                    }
                }
            };
            (cancelled, result, quit)
        };
        if quit {
            if let Err(error) = self.abort("shutdown") {
                self.fail(&error.to_string());
            }
            return false;
        }
        self.last_turn = match &result {
            Some(Ok(turn)) => Some(turn.clone()),
            _ => None,
        };
        self.finish_turn(cancelled, result);
        self.send_settings();
        true
    }

    fn finish_turn<T>(&mut self, cancelled: bool, result: Option<crate::error::Result<T>>) {
        if cancelled || matches!(&result, Some(Err(_))) {
            self.recover();
        }
        if cancelled {
            if let Err(error) = self.abort("cancelled by user") {
                self.fail(&format!("cancellation not recorded: {error}"));
            }
            drop(self.ui.send(Msg::Busy(None)));
        } else if let Some(Err(error)) = result {
            if let Err(abort) = self.abort("turn failed") {
                self.fail(&format!(
                    "{error}; and the turn could not be closed: {abort}"
                ));
            } else {
                self.fail(&format!("{error} · F5 or /retry continues"));
            }
            drop(self.ui.send(Msg::Busy(None)));
        } else {
            self.send_diff(true);
            drop(self.ui.send(Msg::Busy(None)));
            if self.loop_.is_work()
                && self
                    .loop_
                    .work_progress()
                    .is_ok_and(|progress| progress.terminal())
            {
                set_run_mode(&mut self.loop_, &self.ui, crate::RunMode::Auto);
            }
        }
    }

    fn abort(&mut self, reason: &str) -> crate::Result<()> {
        let ui = &self.ui;
        self.loop_
            .abort_with(reason, |event| drop(ui.send(Msg::Event(event.clone()))))
    }

    pub(super) fn recover(&mut self) {
        let mut pending: Vec<UserInput> = self
            .steering
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .collect();
        pending.extend(self.followups.drain(..));
        while let Ok(command) = self.cmd.try_recv() {
            match command {
                Cmd::Submit(input) | Cmd::Steer(input) | Cmd::Queue(input) => pending.push(input),
                command => self.deferred.push_back(command),
            }
        }
        for input in &mut pending {
            if let Some(source) = input.source.take() {
                input.text = source.text;
                input.images.drain(..source.images.min(input.images.len()));
            }
        }
        if !pending.is_empty() {
            drop(self.ui.send(Msg::Restored(pending)));
        }
        super::announce(&self.ui, &self.steering, &self.followups);
    }

    pub(super) fn send_diff(&mut self, announce: bool) {
        let diff = match crate::diff::stats(self.loop_.workspace()) {
            Ok(diff) => diff,
            Err(error) => {
                self.last_diff = None;
                drop(self.ui.send(Msg::DiffFailed(error.to_string())));
                return;
            }
        };
        if announce {
            let calls = self.last_turn.iter().flat_map(|turn| {
                turn.events.iter().filter_map(|event| match event {
                    Event::ToolCall {
                        name, arguments, ..
                    } => Some((name.as_str(), arguments.as_str())),
                    _ => None,
                })
            });
            drop(self.ui.send(Msg::Edited(crate::diff::edited(calls))));
            if self.last_diff.as_ref() == Some(&diff) {
                return;
            }
            if !diff.files.is_empty() {
                drop(self.ui.send(Msg::System(format!(
                    "changes: {} · /pane diff",
                    diff.summary()
                ))));
            }
        }
        self.last_diff = Some(diff.clone());
        drop(self.ui.send(Msg::Diff(diff)));
    }

    pub(super) fn tree_at(
        &self,
        path: &std::path::Path,
    ) -> crate::error::Result<crate::worktree::Tree> {
        crate::worktree::list(self.loop_.workspace())?
            .into_iter()
            .find(|tree| tree.path == path)
            .ok_or_else(|| {
                crate::error::Error::Config(format!("no worktree at {}", path.display()))
            })
    }
}

pub(super) fn mail(ui: &UnboundedSender<Msg>, from: &str, to: &[String], text: &str) {
    let note = match crate::bus::deliver(from, to, text) {
        Ok(_) => format!(
            "sent to {}",
            to.iter()
                .map(|id| format!("@{id}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Err(error) => format!("mail failed: {error}"),
    };
    drop(ui.send(Msg::System(note)));
}
