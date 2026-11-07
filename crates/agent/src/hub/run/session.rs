use std::path::PathBuf;

use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;

use super::super::replay::ReplayState;
use super::super::{Status, seen, status};
use super::title_from;
use crate::hub::{Cmd, Fact, Intent, Msg};

pub(super) struct Session {
    pub(super) id: String,
    pub(super) title: Option<String>,
    pub(super) workspace: PathBuf,
    pub(super) parent: Option<String>,
    pub(super) status: Status,
    pub(super) replay: ReplayState,
    pub(super) stale: bool,
    pub(super) cmd: UnboundedSender<Cmd>,
    pub(super) task: JoinHandle<()>,
}

impl Session {
    pub(super) fn name_from(&mut self, command: &Cmd) -> bool {
        if self.title.is_some() {
            return false;
        }
        self.title = title_from(command);
        self.title.is_some()
    }

    pub(super) fn observe(&mut self, msg: &Msg, active: bool) -> bool {
        let next = status(self.status, msg);
        let settled = if active { seen(next) } else { next };
        let mut changed = self.status != settled;
        self.status = settled;
        if let Msg::Intent(Intent::User(text)) = msg
            && self.title.is_none()
        {
            self.title = Some(text.clone());
            changed = true;
        }
        if let Msg::Fact(Fact::Named(name)) = msg
            && self.title.as_ref() != Some(name)
        {
            self.title = Some(name.clone());
            changed = true;
        }
        if let Msg::Fact(Fact::Session(id)) = msg
            && self.id != *id
        {
            self.id = id.clone();
            changed = true;
        }
        self.replay.record(msg, active);
        changed
    }
}
