use std::collections::BTreeMap;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;

use super::replay::ReplayState;
use super::{Factory, Live, Open, Status, seen};
use crate::hub::{Cmd, Fact, Info, Intent, Msg, Ui, worker};
use crate::provider::Client;
use crate::runtime::Loop;
use crate::workflow::Workflow;

mod session;
mod target;
use session::Session;

pub struct Started {
    pub commands: UnboundedSender<Cmd>,
    pub inbox: UnboundedReceiver<Msg>,
    pub task: JoinHandle<()>,
}

/// One window pane: the session it shows and where its messages go.
struct Pane {
    key: Option<u64>,
    ui: UnboundedSender<Msg>,
}

struct Hub {
    sessions: BTreeMap<u64, Session>,
    panes: BTreeMap<u64, Pane>,
    next: u64,
    next_pane: u64,
    merged: UnboundedSender<(u64, Msg)>,
    attached: UnboundedSender<(u64, Option<Cmd>)>,
    factory: Factory,
    info: Info,
}

/// The first pane, which also gets login prompts and ends the hub when it closes.
const MAIN: u64 = 0;

pub fn start(
    loop_: Loop<Client>,
    info: Info,
    ui: (UnboundedSender<Msg>, UnboundedReceiver<Msg>),
    factory: Factory,
) -> Started {
    let (commands, command_rx) = unbounded_channel();
    let (app, inbox) = unbounded_channel();
    let (merged, merged_rx) = unbounded_channel();
    let (attached, attached_rx) = unbounded_channel();
    let workflow = info.workflow.clone();
    let mut hub = Hub {
        sessions: BTreeMap::new(),
        panes: BTreeMap::from([(MAIN, Pane { key: None, ui: app })]),
        next: 0,
        next_pane: MAIN + 1,
        merged,
        attached,
        factory,
        info,
    };
    let key = hub.spawn(loop_, ui, workflow, false);
    hub.show(MAIN, key);
    hub.publish();
    let task = tokio::spawn(hub.run(command_rx, merged_rx, attached_rx));
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
        mut attached: UnboundedReceiver<(u64, Option<Cmd>)>,
    ) {
        loop {
            tokio::select! {
                command = commands.recv() => {
                    if !command.is_some_and(|command| self.command(MAIN, command)) {
                        break;
                    }
                }
                Some((pane, command)) = attached.recv() => match command {
                    Some(command) => {
                        if !self.command(pane, command) {
                            break;
                        }
                    }
                    None => self.detach(pane),
                },
                Some((key, msg)) = merged.recv() => self.message(key, msg),
            }
        }
        self.quit().await;
    }

    fn command(&mut self, pane: u64, command: Cmd) -> bool {
        if let Cmd::Open(open) = command {
            self.open(pane, open);
            return true;
        }
        if let Cmd::Attach(view) = command {
            self.attach(view);
            return true;
        }
        if matches!(command, Cmd::Detach) {
            if pane != MAIN {
                self.detach(pane);
            }
            return true;
        }
        if matches!(command, Cmd::Quit) {
            return false;
        }
        let Some(command) = self.targeted(pane, command) else {
            return true;
        };
        if matches!(command, Cmd::Tick) {
            let peers = peers();
            for session in self.sessions.values() {
                drop(session.cmd.send(Cmd::Tick));
                drop(session.cmd.send(Cmd::Peers(peers.clone())));
            }
            return true;
        }
        let key = self.panes.get(&pane).and_then(|pane| pane.key);
        if let Some(session) = key.and_then(|key| self.sessions.get_mut(&key)) {
            let named = session.name_from(&command);
            drop(session.cmd.send(command));
            if named {
                self.publish();
            }
        }
        true
    }

    fn message(&mut self, key: u64, msg: Msg) {
        if matches!(
            msg,
            Msg::Fact(Fact::Secret(_)) | Msg::Intent(Intent::OpenLogin(_))
        ) {
            let ui = self.viewer(key).unwrap_or_else(|| self.main());
            drop(ui.send(msg));
            return;
        }
        let viewer = self.viewer(key);
        let Some(session) = self.sessions.get_mut(&key) else {
            return;
        };
        let changed = match &msg {
            Msg::Fact(Fact::Transcript(lines)) => lines.iter().fold(false, |changed, line| {
                session.observe(line, viewer.is_some()) | changed
            }),
            msg => session.observe(msg, viewer.is_some()),
        };
        let finished = matches!(msg, Msg::Fact(Fact::Busy(None)));
        match &viewer {
            None => session.stale |= session.replay.working(),
            Some(ui) if matches!(msg, Msg::Fact(Fact::Replayed)) => session.replay.resend(ui),
            Some(ui) => drop(ui.send(msg)),
        }
        if viewer.is_some() && finished && session.stale {
            session.stale = false;
            drop(session.cmd.send(Cmd::Replay));
        }
        if changed {
            self.publish();
        }
    }

    fn open(&mut self, pane: u64, open: Open) {
        let Some(ui) = self.panes.get(&pane).map(|pane| pane.ui.clone()) else {
            return;
        };
        if let Open::Resume(id) = &open
            && let Some(key) = self.find(id)
        {
            return self.activate(pane, key);
        }
        let (tx, rx) = unbounded_channel();
        let factory = self.factory.clone();
        let open = self.isolated(open);
        let made = tokio::task::block_in_place(|| factory(open, Ui(tx.clone())));
        match made {
            Ok((loop_, workflow)) => {
                drop(ui.send(Msg::Fact(Fact::Busy(None))));
                let key = self.spawn(loop_, (tx, rx), workflow, true);
                self.show(pane, key);
                self.publish();
            }
            Err(error) => drop(ui.send(Msg::Fact(Fact::Failed(error.to_string())))),
        }
    }

    fn activate(&mut self, pane: u64, key: u64) {
        let shown = self
            .panes
            .get(&pane)
            .is_some_and(|shown| shown.key != Some(key));
        if !shown || !self.sessions.contains_key(&key) {
            return;
        }
        self.show(pane, key);
        let Some(session) = self.sessions.get_mut(&key) else {
            return;
        };
        session.status = seen(session.status);
        drop(session.cmd.send(Cmd::Replay));
        self.publish();
    }

    /// Shows `key` in `pane`, and clears it from any other pane, which goes blank.
    fn show(&mut self, pane: u64, key: u64) {
        for (id, other) in &mut self.panes {
            if *id != pane && other.key == Some(key) {
                other.key = None;
                drop(other.ui.send(Msg::Intent(Intent::Reset)));
            }
        }
        if let Some(shown) = self.panes.get_mut(&pane) {
            shown.key = Some(key);
        }
    }

    fn attach(&mut self, view: super::View) {
        let pane = self.next_pane;
        self.next_pane += 1;
        self.panes.insert(
            pane,
            Pane {
                key: None,
                ui: view.ui,
            },
        );
        drop(tokio::spawn(follow(
            pane,
            view.commands,
            self.attached.clone(),
        )));
        self.publish();
    }

    fn detach(&mut self, pane: u64) {
        self.panes.remove(&pane);
        self.publish();
    }

    fn viewer(&self, key: u64) -> Option<UnboundedSender<Msg>> {
        self.panes
            .values()
            .find(|pane| pane.key == Some(key))
            .map(|pane| pane.ui.clone())
    }

    fn main(&self) -> UnboundedSender<Msg> {
        self.panes
            .get(&MAIN)
            .map(|pane| pane.ui.clone())
            .expect("the main pane lives as long as the hub")
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
        replace: bool,
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
        let task = tokio::spawn(worker(loop_, cmd_rx, ui.0, info, replace));
        drop(tokio::spawn(forward(key, ui.1, self.merged.clone())));
        self.sessions.insert(
            key,
            Session {
                id,
                title,
                workspace,
                status: Status::Idle,
                replay: ReplayState::default(),
                stale: false,
                cmd,
                task,
            },
        );
        key
    }

    fn publish(&self) {
        for pane in self.panes.values() {
            let live = self
                .sessions
                .iter()
                .map(|(key, session)| Live {
                    session: session.id.clone(),
                    title: session.title.clone().unwrap_or_default(),
                    workspace: session.workspace.clone(),
                    status: session.status,
                    active: pane.key == Some(*key),
                })
                .collect();
            drop(pane.ui.send(Msg::Fact(Fact::Live(live))));
        }
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

async fn follow(
    pane: u64,
    mut rx: UnboundedReceiver<Cmd>,
    attached: UnboundedSender<(u64, Option<Cmd>)>,
) {
    while let Some(command) = rx.recv().await {
        if attached.send((pane, Some(command))).is_err() {
            return;
        }
    }
    drop(attached.send((pane, None)));
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

fn peers() -> std::sync::Arc<[crate::mail::Agent]> {
    if !crate::mail::listed() {
        return std::sync::Arc::new([]);
    }
    crate::mail::dir().map_or_else(
        |_| std::sync::Arc::new([]) as std::sync::Arc<[_]>,
        |dir| crate::mail::live(&dir).into(),
    )
}
