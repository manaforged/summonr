use std::collections::VecDeque;
use std::sync::{Arc, PoisonError};

use tokio::sync::mpsc::UnboundedSender;

use super::settings::set_run_mode;
use super::{Cmd, Msg, Worker};
use crate::provider::UserInput;
use crate::tools::Event;
use crate::tui::Activity;

pub(in crate::tui) const SENT_NOW: &str = "sent to the running turn";
const SETTINGS_LATER: &str = "settings apply when the current turn ends; esc cancels now";

enum Input {
    User(UserInput),
    Mail,
}

enum Outcome {
    Keep,
    Cancel,
    Quit,
}

struct MidTurn<'a> {
    deferred: &'a mut VecDeque<Cmd>,
    followups: &'a mut VecDeque<UserInput>,
    steering: &'a Arc<crate::runtime::Steering>,
    auto_reply: &'a mut bool,
    ui: &'a UnboundedSender<Msg>,
    reader: &'a super::reader::Reader,
}

impl MidTurn<'_> {
    async fn attend(
        &mut self,
        command: Option<Cmd>,
        session_path: &std::path::Path,
        session_id: &str,
        workspace: &std::path::Path,
    ) -> Outcome {
        match command {
            Some(Cmd::Cancel) => Outcome::Cancel,
            Some(Cmd::Quit) => {
                drop(self.ui.send(Msg::Bye));
                Outcome::Quit
            }
            None => Outcome::Quit,
            Some(command) => {
                self.command(command, session_path, session_id, workspace)
                    .await
            }
        }
    }

    async fn command(
        &mut self,
        command: Cmd,
        session_path: &std::path::Path,
        session_id: &str,
        workspace: &std::path::Path,
    ) -> Outcome {
        match command {
            command @ (Cmd::Command(_)
            | Cmd::Branch { .. }
            | Cmd::Login(_)
            | Cmd::SetKey(..)
            | Cmd::Logout(_)
            | Cmd::Merge(_)
            | Cmd::Discard(_)
            | Cmd::SetVerify(_)
            | Cmd::ToggleApproval
            | Cmd::SetEndpoint(_)
            | Cmd::SetModel(_)
            | Cmd::SetSeat(..)
            | Cmd::SeatRole(..)
            | Cmd::ClearRole(_)
            | Cmd::SaveWorkflow(_)
            | Cmd::ListWorkflows
            | Cmd::SetModule(..)) => self.later(command),
            command @ (Cmd::ListModels { .. }
            | Cmd::Pin(..)
            | Cmd::SaveDefault(_)
            | Cmd::ListSessions
            | Cmd::ListUsage
            | Cmd::DeleteSession(_)
            | Cmd::ListPrompts(_)
            | Cmd::ListFiles
            | Cmd::Jobs
            | Cmd::Mcp
            | Cmd::KillJob(_)
            | Cmd::Clipboard
            | Cmd::Diff(_)
            | Cmd::Patch(..)
            | Cmd::Review { .. }
            | Cmd::Trees
            | Cmd::Offers
            | Cmd::View(..)
            | Cmd::Save(..)) => {
                self.reader.run(command).await;
                Outcome::Keep
            }
            command @ (Cmd::Steer(_)
            | Cmd::Queue(_)
            | Cmd::Submit(_)
            | Cmd::Dequeue
            | Cmd::SteerQueued) => self.queued(command, workspace),
            Cmd::Export(target) => {
                super::export_session(session_path, session_id, target, self.ui);
                Outcome::Keep
            }
            Cmd::SetEffort(effort) => {
                self.deferred.push_back(Cmd::SetEffort(effort));
                Outcome::Keep
            }
            Cmd::SetMail(on) => {
                *self.auto_reply = on;
                Outcome::Keep
            }
            Cmd::Send(to, text) => {
                mail(self.ui, session_id, &to, &text);
                Outcome::Keep
            }
            Cmd::Replay => {
                super::history::reshow_running(session_path, session_id, self.ui);
                Outcome::Keep
            }
            Cmd::Answer(text) => {
                self.steering.answer(UserInput::from(text));
                Outcome::Keep
            }
            Cmd::Mail | Cmd::Tick | Cmd::LoginAnswer(_) | Cmd::Open(_) => Outcome::Keep,
            Cmd::Cancel | Cmd::Quit => unreachable!("attend handles Cancel and Quit"),
        }
    }

    fn later(&mut self, command: Cmd) -> Outcome {
        let note = match &command {
            Cmd::Command(line) => format!(
                "{} runs when the current turn ends; esc cancels now",
                line.split_whitespace().next().unwrap_or(line)
            ),
            _ => SETTINGS_LATER.to_string(),
        };
        self.deferred.push_back(command);
        drop(self.ui.send(Msg::System(note)));
        Outcome::Keep
    }

    fn queued(&mut self, command: Cmd, workspace: &std::path::Path) -> Outcome {
        match command {
            Cmd::Steer(text) => {
                if let Ok(mut queue) = self.steering.lock() {
                    queue.push_back(text);
                    if let Some(notes) = crate::hunks::drain(workspace) {
                        queue.push_back(UserInput::from(notes));
                    }
                }
            }
            Cmd::Queue(input) | Cmd::Submit(input) => self.followups.push_back(input),
            Cmd::SteerQueued => {
                let sent = self.followups.len();
                if sent > 0
                    && let Ok(mut queue) = self.steering.lock()
                {
                    queue.extend(self.followups.drain(..));
                    if let Some(notes) = crate::hunks::drain(workspace) {
                        queue.push_back(UserInput::from(notes));
                    }
                }
                if sent > 0 {
                    drop(self.ui.send(Msg::System(SENT_NOW.to_string())));
                }
            }
            Cmd::Dequeue => {
                let mut pending: Vec<UserInput> = self
                    .steering
                    .lock()
                    .map_or_else(|_| Vec::new(), |mut queue| queue.drain(..).collect());
                pending.extend(self.followups.drain(..));
                if !pending.is_empty() {
                    drop(self.ui.send(Msg::Restored(pending)));
                }
            }
            _command => unreachable!("queued routes only queue commands"),
        }
        super::announce(self.ui, self.steering, self.followups);
        Outcome::Keep
    }
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
            let workspace = self.loop_.workspace().to_path_buf();
            let turn = async {
                match input {
                    Input::User(input) => self.loop_.turn_with(input, emit).await,
                    Input::Mail => self.loop_.turn_mail_with(emit).await,
                }
            };
            tokio::pin!(turn);
            let mut mid = MidTurn {
                deferred: &mut self.deferred,
                followups: &mut self.followups,
                steering: &self.steering,
                auto_reply: &mut self.auto_reply,
                ui: &self.ui,
                reader: &self.reader,
            };
            let (mut cancelled, mut quit) = (false, false);
            let result = loop {
                tokio::select! {
                    done = &mut turn => break Some(done),
                    command = self.cmd.recv() => match mid
                        .attend(command, &session_path, &session_id, &workspace)
                        .await
                    {
                        Outcome::Keep => {}
                        Outcome::Cancel => {
                            cancelled = true;
                            break None;
                        }
                        Outcome::Quit => {
                            quit = true;
                            break None;
                        }
                    }
                }
            };
            (cancelled, result, quit)
        };
        if quit {
            if let Err(error) = self.abort(crate::tools::AbortCode::Interrupted, "shutdown") {
                self.fail(&error.to_string());
            }
            return false;
        }
        self.last_turn = match &result {
            Some(Ok(turn)) => Some(turn.clone()),
            _ => None,
        };
        if matches!(&result, Some(Ok(turn)) if !turn.aborted) {
            self.remember_seat();
        }
        self.finish_turn(cancelled, result);
        self.send_settings();
        true
    }

    fn finish_turn<T>(&mut self, cancelled: bool, result: Option<crate::error::Result<T>>) {
        if cancelled || matches!(&result, Some(Err(_))) {
            self.recover();
        }
        if cancelled {
            if let Err(error) = self.abort(crate::tools::AbortCode::Cancelled, "cancelled by user")
            {
                self.fail(&format!("cancellation not recorded: {error}"));
            }
            drop(self.ui.send(Msg::Busy(None)));
        } else if let Some(Err(error)) = result {
            if let Err(abort) = self.abort(crate::tools::AbortCode::Error, "turn failed") {
                self.fail(&format!(
                    "{error}; and the turn could not be closed: {abort}"
                ));
            } else {
                self.fail(&format!("{error} · F5 or /retry continues"));
            }
            drop(self.ui.send(Msg::Busy(None)));
        } else {
            self.carry_steering();
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

    fn abort(&mut self, code: crate::tools::AbortCode, reason: &str) -> crate::Result<()> {
        let ui = &self.ui;
        let abort = crate::tools::Abort {
            code,
            reason: reason.to_string(),
        };
        self.loop_
            .abort_with(&abort, |event| drop(ui.send(Msg::Event(event.clone()))))
    }

    fn carry_steering(&mut self) {
        let left: Vec<UserInput> = self
            .steering
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .collect();
        self.carried = left.len();
        for input in left.into_iter().rev() {
            self.followups.push_front(input);
        }
        super::announce(&self.ui, &self.steering, &self.followups);
    }

    pub(super) fn recover(&mut self) {
        let mut pending: Vec<UserInput> = self
            .steering
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .collect();
        pending.extend(self.followups.drain(..));
        self.carried = 0;
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
}

pub(super) fn mail(ui: &UnboundedSender<Msg>, from: &str, to: &[String], text: &str) {
    let note = match crate::mail::deliver(from, to, text) {
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
