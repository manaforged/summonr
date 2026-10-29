use tokio::sync::mpsc::UnboundedSender;

use super::execution::ExecutionClass;
use super::settings::set_run_mode;
use super::state::set_mail_reply;
use super::{Cmd, Msg, Worker};
use crate::provider::UserInput;
use crate::tools::Event;
use crate::tui::Activity;

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
            let workspace = self.loop_.workspace().to_path_buf();
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
                        None => {
                            quit = true;
                            break None;
                        }
                        Some(Cmd::Cancel) => {
                            cancelled = true;
                            break None;
                        }
                        Some(Cmd::Quit) => {
                            drop(self.ui.send(Msg::Bye));
                            quit = true;
                            break None;
                        }
                        Some(command) if command.execution_class() == ExecutionClass::AfterTurn => {
                            self.queue.defer(command, &self.ui);
                        }
                        Some(command @ (Cmd::Steer(_)
                        | Cmd::Queue(_)
                        | Cmd::Submit(_)
                        | Cmd::Dequeue
                        | Cmd::SteerQueued)) => self.queue.during_turn(command, &workspace, &self.ui),
                        Some(Cmd::Export(target)) => {
                            super::export_session(&session_path, &session_id, target, &self.ui);
                        }
                        Some(Cmd::SetMail(on)) => {
                            set_mail_reply(&mut self.auto_reply, &mut self.pending_mail, on);
                        }
                        Some(Cmd::Send(to, text)) => mail(&self.ui, &session_id, &to, &text),
                        Some(Cmd::Replay) => super::history::replay_transcript(
                            &session_path,
                            Some(session_id.clone()),
                            None,
                            &self.ui,
                        ),
                        Some(Cmd::Answer(dialog, text)) => {
                            self.queue.steering.answer(dialog, UserInput::from(text));
                        }
                        Some(Cmd::Tick) => live_peers(&session_id, &mut self.last_peers, &self.ui),
                        Some(Cmd::Mail | Cmd::LoginAnswer(_) | Cmd::Open(_)) => {}
                        Some(command) => self.reader.run(command).await,
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
        let left = self.queue.take_steering();
        self.queue.carried = left.len();
        for input in left.into_iter().rev() {
            self.queue.followups.push_front(input);
        }
        self.queue.announce(&self.ui);
    }

    pub(super) fn recover(&mut self) {
        let mut pending = self.queue.take_steering();
        pending.extend(self.queue.followups.drain(..));
        self.queue.carried = 0;
        while let Ok(command) = self.cmd.try_recv() {
            match command {
                Cmd::Submit(input) | Cmd::Steer(input) | Cmd::Queue(input) => pending.push(input),
                command => self.queue.deferred.push_back(command),
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
        self.queue.announce(&self.ui);
    }
}

fn live_peers(
    session_id: &str,
    peers: &mut Option<Vec<crate::mail::Agent>>,
    ui: &UnboundedSender<Msg>,
) {
    if !crate::mail::listed() {
        return;
    }
    match crate::mail::dir() {
        Ok(dir) => super::state::publish_peers(crate::mail::live(&dir), session_id, peers, ui),
        Err(error) => drop(ui.send(Msg::System(error.to_string()))),
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
