use std::sync::{Mutex, PoisonError};

use crate::hub::Cmd;

/// One background reload: at most one runs, and only the newest request for each target waits behind it.
#[derive(Default)]
pub(super) struct Load(Mutex<State>);

#[derive(Default)]
struct State {
    running: bool,
    queued: Vec<Cmd>,
}

impl Load {
    pub(super) fn claim(&self, command: Cmd) -> Option<Cmd> {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if state.running {
            state
                .queued
                .retain(|waiting| !same_target(waiting, &command));
            state.queued.push(command);
            return None;
        }
        state.running = true;
        Some(command)
    }

    pub(super) fn next(&self) -> Option<Cmd> {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let next = (!state.queued.is_empty()).then(|| state.queued.remove(0));
        state.running = next.is_some();
        next
    }
}

fn same_target(a: &Cmd, b: &Cmd) -> bool {
    if let (Cmd::RunPanel(a), Cmd::RunPanel(b)) = (a, b) {
        return a == b;
    }
    std::mem::discriminant(a) == std::mem::discriminant(b)
}
