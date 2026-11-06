use super::{Hub, Status};
use crate::hub::{Cmd, Fact, Msg};

impl Hub {
    /// Runs a command aimed at one session; returns any other command.
    pub(super) fn targeted(&mut self, pane: u64, command: Cmd) -> Option<Cmd> {
        match command {
            Cmd::Stop(id) => self.stop(&id),
            Cmd::Rename { session, name } => self.rename(pane, &session, &name),
            Cmd::DeleteSession(id) => self.delete(pane, id),
            other => return Some(other),
        }
        None
    }

    fn stop(&self, id: &str) {
        if let Some(session) = self.find(id).and_then(|key| self.sessions.get(&key)) {
            drop(session.cmd.send(Cmd::Cancel));
        }
    }

    fn rename(&self, pane: u64, id: &str, name: &str) {
        if let Some(session) = self.find(id).and_then(|key| self.sessions.get(&key)) {
            drop(session.cmd.send(Cmd::Command(format!("/name {name}"))));
            return;
        }
        let named = tokio::task::block_in_place(|| {
            crate::session::resolve_id(id).and_then(|path| crate::session::set_name(&path, name))
        });
        if let Err(error) = named {
            self.tell(pane, error.to_string());
        }
    }

    fn delete(&mut self, pane: u64, id: String) {
        let Some(key) = self.find(&id) else {
            return self.relay(pane, Cmd::DeleteSession(id));
        };
        let shown = self.panes.values().any(|pane| pane.key == Some(key));
        let running = self
            .sessions
            .get(&key)
            .is_some_and(|session| matches!(session.status, Status::Working | Status::NeedsYou));
        if shown || running {
            return self.tell(
                pane,
                "Stop this agent and close its pane to delete it.".to_owned(),
            );
        }
        let Some(session) = self.sessions.remove(&key) else {
            return;
        };
        drop(session.cmd.send(Cmd::Quit));
        self.publish();
        let attached = self.attached.clone();
        drop(tokio::spawn(async move {
            drop(session.task.await);
            drop(attached.send((pane, Some(Cmd::DeleteSession(id)))));
        }));
    }

    /// Sends a catalog command to the pane's session, or any session when the pane is blank.
    fn relay(&self, pane: u64, command: Cmd) {
        let shown = self.panes.get(&pane).and_then(|pane| pane.key);
        let session = shown
            .and_then(|key| self.sessions.get(&key))
            .or_else(|| self.sessions.values().next());
        match session {
            Some(session) => drop(session.cmd.send(command)),
            None => self.tell(pane, "No agent is running to do that.".to_owned()),
        }
    }

    fn tell(&self, pane: u64, text: String) {
        if let Some(pane) = self.panes.get(&pane) {
            drop(pane.ui.send(Msg::Fact(Fact::Failed(text))));
        }
    }
}
