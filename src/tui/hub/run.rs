use std::collections::BTreeMap;
use std::path::PathBuf;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

use super::sticky::Sticky;
use super::{Factory, Live, Open, Status, seen, status};
use crate::provider::Client;
use crate::runtime::Loop;
use crate::tui::{Cmd, Info, Msg, Ui, worker};
use crate::workflow::Workflow;

pub(in crate::tui) struct Started {
    pub(in crate::tui) commands: UnboundedSender<Cmd>,
    pub(in crate::tui) inbox: UnboundedReceiver<Msg>,
    pub(in crate::tui) task: JoinHandle<()>,
}

struct Session {
    id: String,
    title: Option<String>,
    workspace: PathBuf,
    status: Status,
    sticky: Sticky,
    stale: bool,
    cmd: UnboundedSender<Cmd>,
    task: JoinHandle<()>,
}

struct Hub {
    sessions: BTreeMap<u64, Session>,
    active: u64,
    next: u64,
    app: UnboundedSender<Msg>,
    merged: UnboundedSender<(u64, Msg)>,
    factory: Factory,
    info: Info,
}

pub(in crate::tui) fn start(
    loop_: Loop<Client>,
    info: Info,
    ui: (UnboundedSender<Msg>, UnboundedReceiver<Msg>),
    factory: Factory,
) -> Started {
    let (commands, command_rx) = unbounded_channel();
    let (app, inbox) = unbounded_channel();
    let (merged, merged_rx) = unbounded_channel();
    let workflow = info.workflow.clone();
    let mut hub = Hub {
        sessions: BTreeMap::new(),
        active: 0,
        next: 0,
        app,
        merged,
        factory,
        info,
    };
    hub.active = hub.spawn(loop_, ui, workflow);
    hub.publish();
    let task = tokio::spawn(hub.run(command_rx, merged_rx));
    Started {
        commands,
        inbox,
        task,
    }
}

impl Hub {
    async fn run(
        mut self,
        mut commands: UnboundedReceiver<Cmd>,
        mut merged: UnboundedReceiver<(u64, Msg)>,
    ) {
        loop {
            tokio::select! {
                command = commands.recv() => {
                    if !command.is_some_and(|command| self.command(command)) {
                        break;
                    }
                }
                Some((key, msg)) = merged.recv() => self.message(key, msg),
            }
        }
        self.quit().await;
    }

    fn command(&mut self, command: Cmd) -> bool {
        if let Cmd::Open(open) = command {
            self.open(open);
            return true;
        }
        if matches!(command, Cmd::Quit) {
            return false;
        }
        if matches!(command, Cmd::Tick) {
            for session in self.sessions.values() {
                drop(session.cmd.send(Cmd::Tick));
            }
            return true;
        }
        if let Some(session) = self.sessions.get_mut(&self.active) {
            let named = session.name_from(&command);
            drop(session.cmd.send(command));
            if named {
                self.publish();
            }
        }
        true
    }

    fn message(&mut self, key: u64, msg: Msg) {
        if matches!(msg, Msg::Secret(_) | Msg::OpenLogin(_)) {
            drop(self.app.send(msg));
            return;
        }
        let active = key == self.active;
        let Some(session) = self.sessions.get_mut(&key) else {
            return;
        };
        let changed = session.observe(&msg, active);
        let finished = matches!(msg, Msg::Busy(None));
        if !active {
            session.stale |= session.sticky.working();
        } else if matches!(msg, Msg::Replayed) {
            session.sticky.resend(&self.app);
        } else {
            drop(self.app.send(msg));
        }
        if active && finished && session.stale {
            session.stale = false;
            drop(self.app.send(Msg::Reset));
            drop(session.cmd.send(Cmd::Replay));
        }
        if changed {
            self.publish();
        }
    }

    fn open(&mut self, open: Open) {
        if let Open::Resume(id) = &open
            && let Some(key) = self.find(id)
        {
            return self.activate(key);
        }
        let (tx, rx) = unbounded_channel();
        let factory = self.factory.clone();
        let made = tokio::task::block_in_place(|| factory(open, Ui(tx.clone())));
        match made {
            Ok((loop_, workflow)) => {
                drop(self.app.send(Msg::Reset));
                drop(self.app.send(Msg::Busy(None)));
                self.active = self.spawn(loop_, (tx, rx), workflow);
                self.publish();
            }
            Err(error) => drop(self.app.send(Msg::Failed(error.to_string()))),
        }
    }

    fn activate(&mut self, key: u64) {
        if key == self.active {
            return;
        }
        let Some(session) = self.sessions.get_mut(&key) else {
            return;
        };
        self.active = key;
        session.status = seen(session.status);
        drop(self.app.send(Msg::Reset));
        drop(session.cmd.send(Cmd::Replay));
        self.publish();
    }

    fn find(&self, id: &str) -> Option<u64> {
        self.sessions
            .iter()
            .find(|(_, session)| session.id == id)
            .map(|(key, _)| *key)
    }

    fn spawn(
        &mut self,
        loop_: Loop<Client>,
        ui: (UnboundedSender<Msg>, UnboundedReceiver<Msg>),
        workflow: Option<Workflow>,
    ) -> u64 {
        let key = self.next;
        self.next += 1;
        let id = loop_.session_id();
        let title = loop_.name();
        let workspace = loop_.workspace().to_path_buf();
        let mut info = self.info.clone();
        info.session.clone_from(&id);
        info.workflow = workflow;
        let (cmd, cmd_rx) = unbounded_channel();
        let task = tokio::spawn(worker(loop_, cmd_rx, ui.0, info));
        drop(tokio::spawn(forward(key, ui.1, self.merged.clone())));
        self.sessions.insert(
            key,
            Session {
                id,
                title,
                workspace,
                status: Status::Idle,
                sticky: Sticky::default(),
                stale: false,
                cmd,
                task,
            },
        );
        key
    }

    fn publish(&self) {
        let live = self
            .sessions
            .iter()
            .map(|(key, session)| Live {
                session: session.id.clone(),
                title: session.title.clone().unwrap_or_default(),
                workspace: session.workspace.clone(),
                status: session.status,
                active: *key == self.active,
            })
            .collect();
        drop(self.app.send(Msg::Live(live)));
    }

    async fn quit(self) {
        for session in self.sessions.values() {
            drop(session.cmd.send(Cmd::Quit));
        }
        for session in self.sessions.into_values() {
            drop(session.task.await);
        }
    }
}

impl Session {
    fn name_from(&mut self, command: &Cmd) -> bool {
        if self.title.is_some() {
            return false;
        }
        self.title = title_from(command);
        self.title.is_some()
    }

    fn observe(&mut self, msg: &Msg, active: bool) -> bool {
        let next = status(self.status, msg);
        let settled = if active { seen(next) } else { next };
        let mut changed = self.status != settled;
        self.status = settled;
        if let Msg::User(text) = msg
            && self.title.is_none()
        {
            self.title = Some(text.clone());
            changed = true;
        }
        if let Msg::Named(name) = msg
            && self.title.as_ref() != Some(name)
        {
            self.title = Some(name.clone());
            changed = true;
        }
        if let Msg::Session(id) = msg
            && self.id != *id
        {
            self.id = id.clone();
            changed = true;
        }
        self.sticky.record(msg, active);
        changed
    }
}

async fn forward(key: u64, mut rx: UnboundedReceiver<Msg>, merged: UnboundedSender<(u64, Msg)>) {
    while let Some(msg) = rx.recv().await {
        if merged.send((key, msg)).is_err() {
            break;
        }
    }
}

pub(super) fn title_from(command: &Cmd) -> Option<String> {
    let Cmd::Submit(input) = command else {
        return None;
    };
    input
        .text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}
