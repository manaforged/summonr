use tokio::sync::mpsc::UnboundedSender;

use super::execution::ExecutionClass;
use super::settings::set_run_mode;
use super::state::{publish_peers, set_mail_reply};
use super::{Cmd, Msg, Worker};
use crate::hub::Activity;
use crate::hub::{Fact, Intent};
use crate::provider::UserInput;
use crate::tools::Event;

enum Input {
    User(UserInput),
    Mail,
}

/// What an agent's transcript says when the app quit during its turn.
pub(super) const QUIT_NOTE: &str = "Stopped at quit";

impl Worker {
    pub(super) async fn turn(&mut self, input: UserInput) -> bool {
        self.mail_held = false;
        self.run_turn(Input::User(input)).await
    }

    pub(super) async fn mail(&mut self) -> bool {
        self.run_turn(Input::Mail).await
    }

    async fn run_turn(&mut self, input: Input) -> bool {
        let request = match &input {
            Input::User(input) => Some(input.text.clone()),
            Input::Mail => None,
        };
        drop(self.ui.send(Msg::Fact(Fact::Busy(Some(Activity::Turn)))));
        let sender = self.ui.clone();
        let delivered = std::sync::Arc::new(tokio::sync::Notify::new());
        let arrived = std::sync::Arc::clone(&delivered);
        let emit = move |event: &Event| {
            if matches!(event, Event::Steered { .. }) {
                delivered.notify_one();
            }
            drop(sender.send(Msg::Fact(Fact::Event(event.clone()))));
        };
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
                    biased;
                    command = self.cmd.recv() => match command {
                        None => {
                            quit = true;
                            break None;
                        }
                        Some(Cmd::Cancel) => {
                            cancelled = true;
                            break None;
                        }
                        Some(Cmd::CancelLogin) => self.queue.drop_auth(),
                        Some(Cmd::Quit) => {
                            drop(self.ui.send(Msg::Intent(Intent::Bye)));
                            quit = true;
                            break None;
                        }
                        Some(Cmd::Command(line)) if crate::parse_command(&line) == Some(crate::SlashCommand::Stop) => {
                            self.queue.defer(Cmd::Command(line), &self.ui);
                            cancelled = true;
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
                        Some(Cmd::Tick) => self.queue.announce(&self.ui),
                        Some(Cmd::Peers(peers)) => {
                            publish_peers(&peers, &session_id, &mut self.last_peers, &self.ui);
                        }
                        Some(Cmd::Mail | Cmd::LoginAnswer(_) | Cmd::Open(_)) => {}
                        Some(command) => self.reader.run(command).await,
                    },
                    () = arrived.notified() => self.queue.announce(&self.ui),
                    done = &mut turn => break Some(done),
                }
            };
            (cancelled, result, quit)
        };
        if quit {
            if let Err(error) = self.abort(crate::tools::AbortCode::Interrupted, QUIT_NOTE) {
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
        let reply = match &result {
            Some(Ok(turn)) if !turn.aborted => Some(turn.text.clone()),
            Some(Ok(_) | Err(_)) | None => None,
        };
        self.finish_turn(cancelled, result).await;
        self.send_settings();
        if let (Some(request), Some(reply)) = (request, reply) {
            self.title_session(&request, &reply).await;
        }
        true
    }

    async fn finish_turn<T: Send>(
        &mut self,
        cancelled: bool,
        result: Option<summonr_core::error::Result<T>>,
    ) {
        if cancelled || matches!(&result, Some(Err(_))) {
            self.recover();
        }
        if cancelled {
            self.mail_held = true;
            self.pending_mail = false;
            if let Err(error) = self.abort(crate::tools::AbortCode::Cancelled, "cancelled by user")
            {
                self.fail(&format!("cancellation not recorded: {error}"));
            }
            drop(self.ui.send(Msg::Fact(Fact::Busy(None))));
        } else if let Some(Err(error)) = result {
            self.fail(&format!("{error}{}", crate::hub::RETRY_HINT));
            drop(self.ui.send(Msg::Fact(Fact::Busy(None))));
        } else {
            self.carry_steering();
            self.send_diff(true).await;
            drop(self.ui.send(Msg::Fact(Fact::Busy(None))));
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

    pub(super) fn stop_all(&mut self) -> bool {
        match self.abort(crate::tools::AbortCode::Interrupted, "stopped by user") {
            Ok(()) => self.system("stopped background agents and jobs"),
            Err(error) => self.fail(&format!("stop failed: {error}")),
        }
        true
    }

    fn abort(&mut self, code: crate::tools::AbortCode, reason: &str) -> crate::Result<()> {
        let ui = &self.ui;
        let abort = crate::tools::Abort {
            code,
            reason: reason.to_string(),
        };
        self.loop_.abort_with(&abort, |event| {
            drop(ui.send(Msg::Fact(Fact::Event(event.clone()))))
        })
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
        pending.extend(self.queue.take(|_| true));
        self.queue.carried = 0;
        self.queue.recorded.clear();
        while let Ok(command) = self.cmd.try_recv() {
            match command {
                Cmd::Submit(input) | Cmd::Steer(input) | Cmd::Queue(input) => pending.push(input),
                Cmd::CancelLogin => self.queue.drop_auth(),
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
            drop(self.ui.send(Msg::Fact(Fact::Restored(pending))));
        }
        self.queue.announce(&self.ui);
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
    drop(ui.send(Msg::Fact(Fact::System(note))));
}

#[cfg(test)]
#[path = "turn_tests.rs"]
mod tests;
