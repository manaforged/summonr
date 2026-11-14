use std::path::PathBuf;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

use super::super::replay::Replay;
use super::super::{Status, seen, status};
use super::{Hub, title_from};
use crate::hub::{Cmd, Fact, Msg, worker};
use crate::provider::Client;
use crate::runtime::Loop;
use crate::tools::Event;
use crate::workflow::Workflow;

pub(super) struct Session {
    pub(super) id: String,
    pub(super) title: Option<String>,
    pub(super) workspace: PathBuf,
    pub(super) parent: Option<String>,
    pub(super) status: Status,
    pub(super) replay: Replay,
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
        let user = match msg {
            Msg::Fact(Fact::Event(event)) => Some(event),
            Msg::Fact(Fact::Transcript { events, .. }) => events
                .iter()
                .find(|event| matches!(event, Event::User { .. })),
            _ => None,
        };
        if let Some(Event::User { text, .. }) = user
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

impl Hub {
    pub(super) fn spawn(
        &mut self,
        loop_: Loop<Client>,
        ui: (UnboundedSender<Msg>, UnboundedReceiver<Msg>),
        workflow: Option<Workflow>,
        replace: bool,
    ) -> u64 {
        let key = self.next;
        self.next += 1;
        let id = loop_.session_id();
        let title = loop_.name();
        let workspace = loop_.workspace().to_path_buf();
        let parent = loop_.parent();
        let mut info = self.info.clone();
        info.session.clone_from(&id);
        info.workflow = workflow;
        let (cmd, cmd_rx) = unbounded_channel();
        let task = tokio::spawn(worker(loop_, cmd_rx, ui.0, info, replace));
        drop(tokio::spawn(forward(key, ui.1, self.merged.clone())));
        self.sessions.insert(
            key,
            Session {
                id,
                title,
                workspace,
                parent,
                status: Status::Idle,
                replay: Replay::default(),
                stale: false,
                cmd,
                task,
            },
        );
        key
    }
}

async fn forward(key: u64, mut rx: UnboundedReceiver<Msg>, merged: UnboundedSender<(u64, Msg)>) {
    while let Some(msg) = rx.recv().await {
        if merged.send((key, msg)).is_err() {
            break;
        }
    }
}
