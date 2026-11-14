use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::Value;

use crate::hooks::{Call, Hook, HookEvent, Permission, Start, Verdict};

#[derive(Default)]
pub(in crate::runtime) struct Pending {
    pub(super) opening: Opening,
    late: Arc<Mutex<Vec<String>>>,
    running: Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

#[derive(Default)]
pub(super) enum Opening {
    #[default]
    Fresh,
    Due(Start),
    Held(Verdict),
    Done,
}

impl Pending {
    pub(in crate::runtime) fn reopen(&mut self, start: Start) {
        self.opening = Opening::Due(start);
    }

    pub(super) fn spawn(&self, hook: Hook, call: &Call<'_>) {
        let sink = Arc::clone(&self.late);
        let owned = Owned::of(call);
        let task = tokio::spawn(async move {
            let heard = crate::hooks::run(&hook, &owned.call()).await;
            if !heard.context.is_empty() {
                sink.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .extend(heard.context);
            }
        });
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        running.retain(|task| !task.is_finished());
        running.push(task);
    }

    #[cfg(test)]
    pub(in crate::runtime) async fn settled(&self) {
        let running =
            std::mem::take(&mut *self.running.lock().unwrap_or_else(PoisonError::into_inner));
        for task in running {
            task.await.expect("a background hook task finishes");
        }
    }

    pub(super) fn take_late(&self) -> Vec<String> {
        std::mem::take(&mut *self.late.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

struct Owned {
    event: HookEvent,
    workspace: PathBuf,
    session: String,
    transcript: PathBuf,
    permission: Permission,
    payload: Value,
    env: Vec<(String, String)>,
}

impl Owned {
    fn of(call: &Call<'_>) -> Self {
        Self {
            event: call.event,
            workspace: call.workspace.to_path_buf(),
            session: call.session.to_string(),
            transcript: call.transcript.to_path_buf(),
            permission: call.permission,
            payload: call.payload.clone(),
            env: call.env.to_vec(),
        }
    }

    fn call(&self) -> Call<'_> {
        Call {
            event: self.event,
            workspace: &self.workspace,
            session: &self.session,
            transcript: &self.transcript,
            permission: self.permission,
            payload: &self.payload,
            env: &self.env,
        }
    }
}
