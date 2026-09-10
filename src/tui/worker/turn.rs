use std::sync::PoisonError;

use super::{Cmd, Msg, Worker, set_mode};
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
        let (cancelled, result, mode, quit) = {
            let turn = async {
                match input {
                    Input::User(input) => self.loop_.turn_with(input, emit).await,
                    Input::Mail => self.loop_.turn_mail_with(emit).await,
                }
            };
            tokio::pin!(turn);
            let (mut cancelled, mut mode, mut quit) = (false, None, false);
            let result = loop {
                tokio::select! {
                    done = &mut turn => break Some(done),
                    command = self.cmd.recv() => match command {
                        Some(command @ (Cmd::Command(_) | Cmd::Branch { .. })) => {
                            self.deferred.push_back(command);
                            drop(self.ui.send(Msg::System(SETTINGS_LATER.to_string())));
                        }
                        Some(command @ (Cmd::ListSessions | Cmd::ListPrompts(_) | Cmd::ListFiles | Cmd::Diff(_) | Cmd::Patch(..) | Cmd::Trees | Cmd::View(..) | Cmd::Save(..))) => self.reader.run(command).await,
                        Some(command @ (Cmd::Login(_) | Cmd::SetKey(_, _) | Cmd::Merge(_) | Cmd::Discard(_))) => {
                            self.deferred.push_back(command);
                            drop(self.ui.send(Msg::System(SETTINGS_LATER.to_string())));
                        }
                        Some(Cmd::Cancel) => { cancelled = true; break None; }
                        Some(Cmd::Quit) => { drop(self.ui.send(Msg::Bye)); quit = true; break None; }
                        Some(Cmd::SetMode(plan)) => {
                            mode = Some(plan);
                            drop(self.ui.send(Msg::Mode(plan)));
                            drop(self.ui.send(Msg::System(MODE_LATER.to_string())));
                        }
                        Some(Cmd::SetBus(on)) => {
                            self.auto_reply = on;
                        }
                        Some(Cmd::Steer(text)) => {
                            if let Ok(mut queue) = self.steering.lock() { queue.push_back(text); }
                        }
                        Some(Cmd::Queue(text)) => self.followups.push_back(text),
                        Some(command @ (Cmd::SetRounds(_) | Cmd::SetVerify(_) | Cmd::SetEndpoint(_) | Cmd::SetModel(_) | Cmd::SetSeat(_, _) | Cmd::ListModels | Cmd::SeatRole(..) | Cmd::ClearRole(_) | Cmd::SaveWorkflow(_) | Cmd::ListWorkflows)) => {
                            self.deferred.push_back(command);
                            drop(self.ui.send(Msg::System(SETTINGS_LATER.to_string())));
                        }
                        Some(Cmd::Submit(input)) => {
                            self.followups.push_back(input);
                        }
                        Some(Cmd::Mail) => {}
                        Some(Cmd::Tick) => {}
                        None => { quit = true; break None; }
                    }
                }
            };
            (cancelled, result, mode, quit)
        };
        if quit {
            if let Err(error) = self.loop_.abort_note("shutdown") {
                self.fail(&error.to_string());
            }
            return false;
        }
        self.last_turn = match &result {
            Some(Ok(turn)) => Some(turn.clone()),
            _ => None,
        };
        if let Some(plan) = mode {
            set_mode(&mut self.loop_, &self.ui, plan);
        }
        self.finish_turn(cancelled, result);
        true
    }

    fn finish_turn<T>(&mut self, cancelled: bool, result: Option<crate::error::Result<T>>) {
        if cancelled || matches!(&result, Some(Err(_))) {
            self.recover();
        }
        if cancelled {
            if let Err(error) = self.loop_.abort_note("cancelled by user") {
                self.fail(&format!("cancellation not recorded: {error}"));
            }
            drop(self.ui.send(Msg::Event(Event::Aborted {
                reason: "cancelled by user".to_string(),
            })));
            self.system("F5 or /retry continues this turn. Up recalls your prompt for editing.");
            drop(self.ui.send(Msg::Busy(None)));
        } else if let Some(Err(error)) = result {
            if let Err(abort) = self.loop_.abort_note("turn failed") {
                self.fail(&format!(
                    "{error}; and the turn could not be closed: {abort}"
                ));
            } else {
                self.fail(&error.to_string());
            }
            self.system("F5 or /retry continues this turn. Up recalls your prompt for editing.");
            drop(self.ui.send(Msg::Busy(None)));
        } else {
            self.send_diff(true);
            drop(self.ui.send(Msg::Busy(None)));
        }
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
    }

    /// Sends the workspace diff to the interface. With `announce`, only a
    /// change since the last report is sent, with a transcript line.
    pub(super) fn send_diff(&mut self, announce: bool) {
        let diff = match crate::diff::stats(self.loop_.workspace()) {
            Ok(diff) => diff,
            Err(error) => {
                if !announce {
                    drop(
                        self.ui
                            .send(Msg::System(format!("diff unavailable: {error}"))),
                    );
                }
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
