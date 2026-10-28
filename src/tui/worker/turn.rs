use std::collections::VecDeque;
use std::sync::{Arc, PoisonError};

use tokio::sync::mpsc::UnboundedSender;

use super::execution::ExecutionClass;
use super::settings::set_run_mode;
use super::state::set_mail_reply;
use super::steer::{steer, steerable, take_inputs};
use super::{Cmd, Msg, Worker};
use crate::provider::UserInput;
use crate::tools::Event;
use crate::tui::Activity;

const SENT_NOW: &str = "sent to the running turn";
const NOTHING_TO_SEND: &str = "nothing to send now: shell and slash lines run after the turn";
pub(in crate::tui) const QUEUED_LATER: &str = "queued for the next turn";
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
    pending_mail: &'a mut bool,
    ui: &'a UnboundedSender<Msg>,
    reader: &'a super::reader::Reader,
    peers: &'a mut Option<Vec<crate::mail::Agent>>,
}

impl MidTurn<'_> {
    fn tick(&mut self, session_id: &str) {
        if !crate::mail::listed() {
            return;
        }
        match crate::mail::dir() {
            Ok(dir) => super::state::publish_peers(
                crate::mail::live(&dir),
                session_id,
                self.peers,
                self.ui,
            ),
            Err(error) => drop(self.ui.send(Msg::System(error.to_string()))),
        }
    }

    async fn attend(
        &mut self,
        command: Option<Cmd>,
        session_path: &std::path::Path,
        session_id: &str,
        workspace: &std::path::Path,
    ) -> Outcome {
        let Some(command) = command else {
            return Outcome::Quit;
        };
        match command {
            Cmd::Cancel => Outcome::Cancel,
            Cmd::Quit => {
                drop(self.ui.send(Msg::Bye));
                Outcome::Quit
            }
            command if command.execution_class() == ExecutionClass::AfterTurn => {
                self.later(command)
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
            Cmd::SetMail(on) => {
                set_mail_reply(self.auto_reply, self.pending_mail, on);
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
            Cmd::Answer(dialog, text) => {
                self.steering.answer(dialog, UserInput::from(text));
                Outcome::Keep
            }
            Cmd::Tick => {
                self.tick(session_id);
                Outcome::Keep
            }
            Cmd::Mail | Cmd::LoginAnswer(_) | Cmd::Open(_) => Outcome::Keep,
            command => {
                self.reader.run(command).await;
                Outcome::Keep
            }
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
        self.deferred
            .extend(self.followups.drain(..).map(Cmd::Submit));
        self.deferred.push_back(command);
        drop(self.ui.send(Msg::System(note)));
        super::announce(self.ui, self.steering, self.followups, self.deferred);
        Outcome::Keep
    }

    fn queued(&mut self, command: Cmd, workspace: &std::path::Path) -> Outcome {
        match command {
            Cmd::Steer(input)
                if self.followups.is_empty() && self.deferred.is_empty() && steerable(&input) =>
            {
                steer(self.steering, [input], workspace);
                drop(self.ui.send(Msg::System(SENT_NOW.to_string())));
            }
            Cmd::Steer(input) => {
                self.followups.push_back(input);
                drop(self.ui.send(Msg::System(QUEUED_LATER.to_string())));
            }
            Cmd::Queue(input) | Cmd::Submit(input) => self.followups.push_back(input),
            Cmd::SteerQueued => {
                let now = take_inputs(self.deferred, self.followups, steerable);
                let note = if now.is_empty() {
                    NOTHING_TO_SEND
                } else {
                    steer(self.steering, now, workspace);
                    SENT_NOW
                };
                drop(self.ui.send(Msg::System(note.to_string())));
            }
            Cmd::Dequeue => {
                let pending = take_inputs(self.deferred, self.followups, |_| true);
                if !pending.is_empty() {
                    drop(self.ui.send(Msg::Restored(pending)));
                }
            }
            _command => unreachable!("queued routes only queue commands"),
        }
        super::announce(self.ui, self.steering, self.followups, self.deferred);
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
                pending_mail: &mut self.pending_mail,
                ui: &self.ui,
                reader: &self.reader,
                peers: &mut self.last_peers,
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
            self.remember_model();
        }
        self.finish_turn(cancelled, result);
        self.send_settings();
        true
    }

    fn finish_turn<T>(&mut self, cancelled: bool, result: Option<summonr_core::error::Result<T>>) {
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
                set_run_mode(&mut self.loop_, &self.ui, crate::WorkKind::Single);
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
        let left = self.take_steering();
        self.carried = left.len();
        for input in left.into_iter().rev() {
            self.followups.push_front(input);
        }
        super::announce(&self.ui, &self.steering, &self.followups, &self.deferred);
    }

    fn take_steering(&self) -> Vec<UserInput> {
        let mut steering = self.steering.lock().unwrap_or_else(PoisonError::into_inner);
        steering.drain(..).collect()
    }

    pub(super) fn recover(&mut self) {
        let mut pending = self.take_steering();
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
        super::announce(&self.ui, &self.steering, &self.followups, &self.deferred);
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

#[cfg(test)]
#[path = "turn_tests.rs"]
mod tests;
