//! Asynchronous command worker for the Sigil terminal interface.

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
mod settings;
mod turn;

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
    auto_turns: u32,
    last_auto: Instant,
    steering: Arc<Mutex<VecDeque<UserInput>>>,
    deferred: VecDeque<Cmd>,
    followups: VecDeque<UserInput>,
    /// The last completed turn, for callers that inspect it (goal mode).
    last_turn: Option<crate::Turn>,
    /// The last reported bus failure, so a persistent fault reports once.
    bus_error: Option<String>,
    /// Workflow named at start, seated on the first run tick.
    named: Option<String>,
    /// The seats as the card edits them.
    workflow: Workflow,
    /// The mage home (config's parent) for recents. `None` in tests keeps the
    /// picker's recent list out of the developer's real home.
    home: Option<std::path::PathBuf>,
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
        Self {
            home,
            loop_,
            cmd,
            ui,
            endpoint: info.endpoint,
            relay: info.relay,
            check,
            auto_reply: info.prefs.bus,
            pending_mail: false,
            auto_turns: 0,
            last_auto: Instant::now(),
            steering,
            deferred: VecDeque::new(),
            followups: VecDeque::new(),
            last_turn: None,
            bus_error: None,
            named: info.workflow,
            workflow: Workflow::default(),
        }
    }

    async fn run(mut self) {
        if let Some(name) = self.named.take() {
            self.apply_workflow(&name);
        }
        self.remember_seat();
        self.send_settings();
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
                self.auto_turns += 1;
                Some(Cmd::Mail)
            }
            command => {
                if matches!(&command, Cmd::Submit(_)) {
                    self.auto_turns = 0;
                }
                Some(command)
            }
        }
    }

    async fn dispatch(&mut self, command: Cmd) -> bool {
        match command {
            Cmd::Command(line) => self.command(&line).await.unwrap_or(true),
            Cmd::Login(name) => {
                self.login(&name);
                true
            }
            Cmd::SetKey(name, key) => {
                self.set_key(&name, &key);
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
                self.set_endpoint(&name);
                true
            }
            Cmd::SetModel(model) => {
                self.set_model(&model);
                true
            }
            Cmd::SetSeat(endpoint, model) => {
                self.set_seat(&endpoint, &model);
                true
            }
            Cmd::ListModels => {
                self.list_models();
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
        // An auto-reply turn drains the same mail and displays it as it feeds
        // it to the model. Show it at poll only when no such turn will run, so
        // each message appears exactly once.
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
        if self.auto_turns >= 8 {
            self.auto_reply = false;
            self.auto_turns = 0;
            self.pending_mail = false;
            self.system("bus auto-reply paused after 8 consecutive mail turns — /bus re-enables");
        } else if self.auto_reply {
            self.pending_mail = true;
        }
    }

    fn set_bus(&mut self, on: bool) {
        self.auto_reply = on;
        if on {
            self.auto_turns = 0;
        } else {
            // Disarm any mail turn already queued, so opting out takes effect
            // before the next tick runs a full turn on peer mail.
            self.pending_mail = false;
        }
    }

    fn set_key(&mut self, name: &str, key: &str) {
        match crate::endpoint::save_key(name, key) {
            Ok(()) if name == self.endpoint => {
                self.set_endpoint(name);
                self.system(&format!("{name}: API key saved"));
            }
            Ok(()) => self.system(&format!("{name}: API key saved")),
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
