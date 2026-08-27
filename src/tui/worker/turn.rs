use super::{Cmd, Msg, Worker, set_mode};
use crate::provider::UserInput;
use crate::tools::Event;

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
        drop(self.ui.send(Msg::Busy(true)));
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
                        Some(command @ Cmd::Command(_)) => {
                            self.deferred.push_back(command);
                            drop(self.ui.send(Msg::System(SETTINGS_LATER.to_string())));
                        }
                        Some(command @ (Cmd::Login(_) | Cmd::SetKey(_, _))) => {
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
                            if on { self.auto_turns = 0; }
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
                            // The interface already shows this as sent; queue it
                            // for after the turn instead of dropping it.
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
        if cancelled {
            if let Err(error) = self.loop_.abort_note("cancelled by user") {
                self.fail(&format!("cancellation not recorded: {error}"));
            }
            drop(self.ui.send(Msg::Event(Event::Aborted {
                reason: "cancelled by user".to_string(),
            })));
            drop(self.ui.send(Msg::Busy(false)));
        } else if let Some(Err(error)) = result {
            // A failed turn leaves the driver's turn open; close it so the next
            // prompt is not refused as "a turn is already open".
            if let Err(abort) = self.loop_.abort_note("turn failed") {
                self.fail(&format!(
                    "{error}; and the turn could not be closed: {abort}"
                ));
            } else {
                self.fail(&error.to_string());
            }
            drop(self.ui.send(Msg::Busy(false)));
        } else {
            drop(self.ui.send(Msg::Busy(false)));
        }
    }
}
