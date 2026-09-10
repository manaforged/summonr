use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::{Cmd, Info, Msg};

use crate::provider::Client;
use crate::provider::UserInput;
use crate::runtime::Loop;
use crate::workflow::Workflow;

mod command;
mod history;
mod reader;
mod settings;
mod turn;

use reader::Reader;
use settings::set_mode;

pub(super) async fn worker(
    loop_: Loop<Client>,
    cmd: UnboundedReceiver<Cmd>,
    ui: UnboundedSender<Msg>,
    info: Info,
) {
    Worker::new(loop_, cmd, ui, info).run().await;
}

struct Worker {
    loop_: Loop<Client>,
    cmd: UnboundedReceiver<Cmd>,
    ui: UnboundedSender<Msg>,
    endpoint: String,
    relay: Option<String>,
    check: Option<Vec<String>>,
    auto_reply: bool,
    pending_mail: bool,
    last_auto: Instant,
    steering: Arc<Mutex<VecDeque<UserInput>>>,
    deferred: VecDeque<Cmd>,
    followups: VecDeque<UserInput>,
    last_turn: Option<crate::Turn>,
    bus_error: Option<String>,
    workflow: Workflow,
    dial: Option<crate::Effort>,
    home: Option<std::path::PathBuf>,
    last_diff: Option<crate::diff::Diff>,
    reader: Reader,
}

impl Worker {
    fn new(
        mut loop_: Loop<Client>,
        cmd: UnboundedReceiver<Cmd>,
        ui: UnboundedSender<Msg>,
        info: Info,
    ) -> Self {
        let steering = Arc::new(Mutex::new(VecDeque::new()));
        loop_.steer_with(steering.clone());
        let check = loop_.verify_commands().to_vec();
        let check = (!check.is_empty()).then_some(check);
        let home = info
            .config
            .as_deref()
            .and_then(std::path::Path::parent)
            .map(std::path::Path::to_path_buf);
        let reader = Reader::new(loop_.workspace().to_path_buf(), ui.clone());
        Self {
            home,
            reader,
            loop_,
            cmd,
            ui,
            endpoint: info.endpoint,
            relay: info.relay,
            check,
            auto_reply: info.prefs.bus,
            pending_mail: false,
            last_auto: Instant::now(),
            steering,
            deferred: VecDeque::new(),
            followups: VecDeque::new(),
            last_turn: None,
            bus_error: None,
            last_diff: None,
            workflow: info.workflow.unwrap_or_default(),
            dial: None,
        }
    }

    async fn run(mut self) {
        self.replay();
        if let Some(name) = self.loop_.workflow() {
            self.system(&format!("workflow {name} · {}", self.workflow.summary()));
        }
        self.send_workflow();
        self.remember_seat();
        self.send_settings();
        self.reader.watch();
        while let Some(command) = self.next().await {
            if !self.dispatch(command).await {
                break;
            }
        }
    }

    async fn next(&mut self) -> Option<Cmd> {
        if let Some(command) = self.deferred.pop_front() {
            return Some(command);
        }
        if let Some(next) = self.followups.pop_front() {
            return Some(Cmd::Submit(next));
        }
        let command = self.cmd.recv().await?;
        match command {
            Cmd::Tick if self.pending_mail && self.last_auto.elapsed().as_secs() >= 10 => {
                self.pending_mail = false;
                self.last_auto = Instant::now();
                Some(Cmd::Mail)
            }
            command => Some(command),
        }
    }

    async fn dispatch(&mut self, command: Cmd) -> bool {
        match command {
            Cmd::Branch { session, turn } => self.branch(&session, turn),
            Cmd::Command(line) => self.command(&line).await.unwrap_or(true),
            Cmd::Login(name) => self.login(&name).await,
            Cmd::SetKey(name, key) => {
                self.set_key(&name, &key).await;
                true
            }
            Cmd::Quit => {
                if let Err(error) = self.loop_.abort_note("shutdown") {
                    self.fail(&error.to_string());
                }
                false
            }
            Cmd::Cancel => true,
            Cmd::Steer(text) | Cmd::Queue(text) => {
                self.followups.push_back(text);
                true
            }
            Cmd::Tick => {
                self.tick();
                true
            }
            Cmd::SetMode(plan) => {
                set_mode(&mut self.loop_, &self.ui, plan);
                true
            }
            Cmd::SetRounds(rounds) => {
                self.loop_.set_max_rounds(rounds);
                self.send_settings();
                true
            }
            Cmd::SetVerify(on) => {
                self.set_verify(on);
                true
            }
            Cmd::SetEndpoint(name) => {
                self.set_endpoint(&name).await;
                true
            }
            Cmd::SetModel(model) => {
                self.set_model(&model).await;
                true
            }
            Cmd::SetSeat(endpoint, model) => {
                self.set_seat(&endpoint, &model).await;
                true
            }
            Cmd::ListModels => {
                self.list_models();
                true
            }
            command @ (Cmd::ListSessions
            | Cmd::ListPrompts(_)
            | Cmd::ListFiles
            | Cmd::Diff(_)
            | Cmd::Patch(..)
            | Cmd::Trees
            | Cmd::View(..)
            | Cmd::Save(..)) => {
                self.reader.run(command).await;
                true
            }
            Cmd::Merge(path) => {
                match self.tree_at(&path).and_then(|tree| {
                    crate::worktree::finish(&tree, "merge from the worktrees pane", &[])
                        .map(|_| tree)
                }) {
                    Ok(tree) => drop(self.ui.send(Msg::System(format!(
                        "merged {} into {}",
                        tree.branch, tree.base
                    )))),
                    Err(error) => drop(self.ui.send(Msg::System(format!("merge failed: {error}")))),
                }
                self.reader.trees();
                true
            }
            Cmd::Discard(path) => {
                match self
                    .tree_at(&path)
                    .and_then(|tree| crate::worktree::discard(&tree).map(|()| tree))
                {
                    Ok(tree) => drop(
                        self.ui
                            .send(Msg::System(format!("removed worktree {}", tree.branch))),
                    ),
                    Err(error) => {
                        drop(self.ui.send(Msg::System(format!("worktree kept: {error}"))))
                    }
                }
                self.reader.trees();
                true
            }
            Cmd::SeatRole(role, endpoint, model) => {
                self.seat_role(role, &endpoint, Some(&model));
                true
            }
            Cmd::ClearRole(role) => {
                self.clear_role(role);
                true
            }
            Cmd::SaveWorkflow(name) => {
                self.save_workflow(&name);
                true
            }
            Cmd::ListWorkflows => {
                self.send_workflow();
                true
            }
            Cmd::SetBus(on) => {
                self.set_bus(on);
                true
            }
            Cmd::Mail => self.mail().await,
            Cmd::Submit(input) => self.submit(input).await,
        }
    }

    fn tick(&mut self) {
        for event in self.loop_.child_events() {
            drop(self.ui.send(Msg::Event(event)));
        }
        self.loop_.bus_heartbeat("idle");
        let peers = self
            .loop_
            .bus_agents()
            .into_iter()
            .filter(|agent| agent.id != self.loop_.session_id())
            .count();
        drop(self.ui.send(Msg::Peers(peers)));
        let mail = match self.loop_.bus_poll() {
            Ok(mail) => {
                if self.bus_error.take().is_some() {
                    drop(self.ui.send(Msg::System("bus recovered".to_string())));
                }
                mail
            }
            Err(error) => {
                let text = format!("bus poll failed: {error}");
                if self.bus_error.as_ref() != Some(&text) {
                    drop(self.ui.send(Msg::System(text.clone())));
                    self.bus_error = Some(text);
                }
                Vec::new()
            }
        };
        if mail.is_empty() {
            return;
        }
        self.queue_mail();
        if !self.pending_mail {
            for message in mail {
                drop(self.ui.send(Msg::Mail {
                    from: message.from.clone(),
                    text: message.text.clone(),
                }));
            }
        }
    }

    fn queue_mail(&mut self) {
        if self.auto_reply {
            self.pending_mail = true;
        }
    }

    fn set_bus(&mut self, on: bool) {
        self.auto_reply = on;
        if !on {
            self.pending_mail = false;
        }
    }

    async fn set_key(&mut self, name: &str, key: &str) {
        match crate::endpoint::save_key(name, key) {
            Ok(source) if name == self.endpoint => {
                self.set_endpoint(name).await;
                self.system(&format!("{name}: API key saved in the {}", source.label()));
            }
            Ok(source) => self.system(&format!("{name}: API key saved in the {}", source.label())),
            Err(error) => self.fail(&error.to_string()),
        }
    }

    fn system(&self, text: &str) {
        drop(self.ui.send(Msg::System(text.to_string())));
    }

    fn fail(&self, text: &str) {
        drop(self.ui.send(Msg::Failed(text.to_string())));
    }
}

#[cfg(test)]
#[path = "worker_tests.rs"]
mod tests;
