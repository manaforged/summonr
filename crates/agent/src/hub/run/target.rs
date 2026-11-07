use super::{Hub, Open, Status};
use crate::endpoint::Friend;
use crate::hub::{Cmd, Fact, Msg};

const GAVE_UP: &str = "The other window did not let go of this chat.";

impl Hub {
    /// Runs a command aimed at one session; returns any other command.
    pub(super) fn targeted(&mut self, pane: u64, command: Cmd) -> Option<Cmd> {
        match command {
            Cmd::Stop(id) => self.stop(&id),
            Cmd::MarkUnread(id) => self.mark_unread(&id),
            Cmd::Rename { session, name } => self.rename(pane, &session, &name),
            Cmd::DeleteSession(id) => self.delete(pane, id),
            other => return Some(other),
        }
        None
    }

    /// Opens a new topic in its repository's checkout, or on its own worktree when another topic is in that repository.
    pub(super) fn isolated(&self, open: Open) -> Open {
        let Open::Folder(folder) = open else {
            return open;
        };
        let repo = checkout(&folder);
        let taken = self
            .sessions
            .values()
            .any(|session| session.title.is_some() && checkout(&session.workspace) == repo);
        if taken && crate::worktree::in_git(&repo) {
            Open::Worktree(repo)
        } else {
            Open::Folder(repo)
        }
    }

    /// Asks the window that holds `id` to let go; the next ticks open it here.
    pub(super) fn take_over(&mut self, pane: u64, id: String) {
        let asked = tokio::task::block_in_place(|| {
            let path = crate::session::resolve_id(&id)?;
            crate::durable::log::ask_release(&path)?;
            Ok::<_, crate::Error>(path)
        });
        match asked {
            Ok(path) => {
                let until = std::time::Instant::now() + crate::limits::limits().session_takeover();
                self.taking = Some(super::Taking {
                    pane,
                    id,
                    path,
                    until,
                });
            }
            Err(error) => self.tell(pane, error.to_string()),
        }
    }

    /// Hands a reopened session back to `friend`.
    pub(super) fn summon(&self, key: u64, friend: Option<Friend>) {
        if let (Some(friend), Some(session)) = (friend, self.sessions.get(&key)) {
            drop(session.cmd.send(Cmd::SelectModel(
                friend.route.to_owned(),
                friend.model.to_owned(),
            )));
        }
    }

    /// Tries again to open the chat being taken over.
    pub(super) fn poll_take(&mut self) {
        let Some(taking) = self.taking.take() else {
            return;
        };
        match self.place(taking.pane, Open::Resume(taking.id.clone())) {
            Err(crate::Error::Held) if std::time::Instant::now() < taking.until => {
                self.taking = Some(taking);
                return;
            }
            Err(crate::Error::Held) => {
                self.tell(taking.pane, GAVE_UP.to_owned());
            }
            Err(error) => self.tell(taking.pane, error.to_string()),
            Ok(()) => {}
        }
        crate::durable::log::clear_release(&taking.path);
    }

    /// Drops a session another window took, and moves its view to a new chat.
    pub(super) fn taken_over(&mut self, key: u64) {
        if self.sessions.remove(&key).is_none() {
            return;
        }
        let shown: Vec<u64> = self
            .panes
            .iter()
            .filter(|(_, pane)| pane.key == Some(key))
            .map(|(id, _)| *id)
            .collect();
        for pane in shown {
            if let Some(view) = self.panes.get_mut(&pane) {
                view.key = None;
                drop(view.ui.send(Msg::Fact(Fact::TakenOver)));
            }
            self.open(pane, Open::New);
        }
        self.publish();
    }

    fn stop(&self, id: &str) {
        if let Some(session) = self.find(id).and_then(|key| self.sessions.get(&key)) {
            drop(session.cmd.send(Cmd::Cancel));
        }
    }

    fn mark_unread(&mut self, id: &str) {
        let Some(session) = self.find(id).and_then(|key| self.sessions.get_mut(&key)) else {
            return;
        };
        if session.status == Status::Idle {
            session.status = Status::Review;
            self.publish();
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

fn checkout(folder: &std::path::Path) -> std::path::PathBuf {
    crate::worktree::checkout_of(folder)
        .filter(|checkout| checkout.is_dir())
        .unwrap_or_else(|| folder.to_path_buf())
}

/// The friend a reopened chat recorded.
pub(super) fn friend_of(open: &Open) -> Option<Friend> {
    let Open::Resume(id) = open else {
        return None;
    };
    let path = crate::session::resolve_id(id).ok()?;
    Friend::of(&crate::session::info(path).agent?)
}
