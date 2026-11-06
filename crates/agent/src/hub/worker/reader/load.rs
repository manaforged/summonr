use std::sync::{Mutex, PoisonError};

use crate::hub::Cmd;

/// One background reload: at most one runs, and only the newest request waits behind it.
#[derive(Default)]
pub(super) struct Load(Mutex<State>);

#[derive(Default)]
struct State {
    running: bool,
    queued: Option<Cmd>,
}

impl Load {
    pub(super) fn claim(&self, command: Cmd) -> Option<Cmd> {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if state.running {
            state.queued = Some(command);
            return None;
        }
        state.running = true;
        Some(command)
    }

    pub(super) fn next(&self) -> Option<Cmd> {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let next = state.queued.take();
        state.running = next.is_some();
        next
    }
}
