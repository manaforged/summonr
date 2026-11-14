use std::collections::VecDeque;
use std::sync::{Arc, PoisonError};

use crate::hub::Fact;
use crate::tools::Event;
use tokio::sync::mpsc::UnboundedSender;

use super::{Cmd, Msg};
use crate::provider::UserInput;

const SENT_NOW: &str = "sent to the running turn";
const NOTHING_TO_SEND: &str = "nothing to send now: shell and slash lines run after the turn";
pub(crate) const QUEUED_LATER: &str = "queued for the next turn";
const SETTINGS_LATER: &str = "settings apply when the current turn ends; esc cancels now";

pub(super) struct InputQueue {
    pub(super) deferred: VecDeque<Cmd>,
    pub(super) followups: VecDeque<UserInput>,
    pub(super) steering: Arc<crate::runtime::Steering>,
    pub(super) carried: usize,
    announced: Vec<(bool, String)>,
    pub(super) recorded: Vec<String>,
}

impl InputQueue {
    pub(super) fn new(steering: Arc<crate::runtime::Steering>) -> Self {
        Self {
            deferred: VecDeque::new(),
            followups: VecDeque::new(),
            steering,
            carried: 0,
            announced: Vec::new(),
            recorded: Vec::new(),
        }
    }

    pub(super) fn announce(&mut self, ui: &UnboundedSender<Msg>) {
        let rows = self.pending_rows();
        if rows != self.announced {
            self.announced.clone_from(&rows);
            drop(ui.send(Msg::Fact(Fact::Pending(rows.into()))));
        }
        let steering = self.steering.lock().map_or(0, |queue| queue.len());
        let held = self
            .deferred
            .iter()
            .filter(|command| matches!(command, Cmd::Submit(_)))
            .count();
        drop(ui.send(Msg::Fact(Fact::Queued {
            steering,
            queued: self.followups.len() + held,
        })));
    }

    fn pending_rows(&self) -> Vec<(bool, String)> {
        let steering = self.steering.lock().unwrap_or_else(PoisonError::into_inner);
        let sent = steering
            .iter()
            .filter(|input| input.source.is_some())
            .map(|input| (true, input.text.clone()));
        let deferred = self.deferred.iter().filter_map(|command| match command {
            Cmd::Submit(input) => Some((false, input.text.clone())),
            _other => None,
        });
        let later = self
            .followups
            .iter()
            .map(|input| (false, input.text.clone()));
        sent.chain(deferred).chain(later).collect()
    }

    pub(super) fn take(&mut self, pick: impl Fn(&UserInput) -> bool) -> Vec<UserInput> {
        let mut taken = Vec::new();
        self.deferred.retain(|command| match command {
            Cmd::Submit(input) if pick(input) => {
                taken.push(input.clone());
                false
            }
            _other => true,
        });
        self.followups.retain(|input| {
            let take = pick(input);
            if take {
                taken.push(input.clone());
            }
            !take
        });
        for input in &taken {
            self.take_recorded(&input.text);
        }
        taken
    }

    pub(super) fn take_recorded(&mut self, text: &str) -> bool {
        let at = self.recorded.iter().position(|shown| shown == text);
        at.map(|at| self.recorded.remove(at)).is_some()
    }

    fn take_sent(&self) -> Vec<UserInput> {
        let mut steering = self.steering.lock().unwrap_or_else(PoisonError::into_inner);
        let (sent, notes) = steering
            .drain(..)
            .partition::<Vec<_>, _>(|input| input.source.is_some());
        steering.extend(notes);
        sent
    }

    pub(super) fn take_steering(&self) -> Vec<UserInput> {
        let mut steering = self.steering.lock().unwrap_or_else(PoisonError::into_inner);
        steering.drain(..).collect()
    }

    pub(super) fn drop_auth(&mut self) {
        self.deferred
            .retain(|command| !matches!(command, Cmd::Login(_) | Cmd::SetKey(..) | Cmd::Logout(_)));
    }

    pub(super) fn defer(&mut self, command: Cmd, ui: &UnboundedSender<Msg>) {
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
        drop(ui.send(Msg::Fact(Fact::Event(Event::Note { text: note }))));
        self.announce(ui);
    }

    pub(super) fn during_turn(
        &mut self,
        command: Cmd,
        workspace: &std::path::Path,
        ui: &UnboundedSender<Msg>,
    ) {
        match command {
            Cmd::Steer(input)
                if self.followups.is_empty() && self.deferred.is_empty() && steerable(&input) =>
            {
                steer(&self.steering, [input], workspace);
                drop(ui.send(Msg::Fact(Fact::Event(Event::Note {
                    text: SENT_NOW.to_string(),
                }))));
            }
            Cmd::Steer(input) => {
                self.followups.push_back(input);
                drop(ui.send(Msg::Fact(Fact::Event(Event::Note {
                    text: QUEUED_LATER.to_string(),
                }))));
            }
            Cmd::Queue(input) => self.followups.push_back(input),
            Cmd::Submit(input) => {
                self.recorded.push(input.text.clone());
                self.followups.push_back(input);
            }
            Cmd::SteerQueued => {
                let now = self.take(steerable);
                let note = if now.is_empty() {
                    NOTHING_TO_SEND
                } else {
                    steer(&self.steering, now, workspace);
                    SENT_NOW
                };
                drop(ui.send(Msg::Fact(Fact::Event(Event::Note {
                    text: note.to_string(),
                }))));
            }
            Cmd::Dequeue => {
                let mut pending = self.take_sent();
                pending.extend(self.take(|_| true));
                if !pending.is_empty() {
                    drop(ui.send(Msg::Fact(Fact::Restored(pending))));
                }
            }
            _command => unreachable!("during_turn routes only queue commands"),
        }
        self.announce(ui);
    }
}

pub(super) fn steerable(input: &UserInput) -> bool {
    let line = input.text.trim_start();
    input.images.is_empty()
        && super::command::shell::shell_prefix(line).is_none()
        && !line.starts_with('/')
}

fn steer(
    steering: &crate::runtime::Steering,
    inputs: impl IntoIterator<Item = UserInput>,
    workspace: &std::path::Path,
) {
    let notes = crate::hunks::drain(workspace);
    let inputs = inputs.into_iter().map(|mut input| {
        input.source.get_or_insert_with(|| crate::provider::Source {
            text: input.text.clone(),
            images: input.images.len(),
        });
        input
    });
    steering.push(inputs.chain(notes.map(UserInput::from)));
}
