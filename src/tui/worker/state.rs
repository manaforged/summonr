use super::export::export_session;
use super::*;

impl Worker {
    pub(super) fn new(
        mut loop_: Loop<Client>,
        cmd: UnboundedReceiver<Cmd>,
        ui: UnboundedSender<Msg>,
        info: Info,
    ) -> Self {
        let steering = Arc::new(Mutex::new(VecDeque::new()));
        loop_.steer_with(steering.clone());
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
            auto_reply: info.prefs.bus,
            pending_mail: false,
            last_auto: Instant::now(),
            steering,
            deferred: VecDeque::new(),
            followups: VecDeque::new(),
            last_turn: None,
            bus_error: None,
            last_diff: None,
            last_peers: None,
            workflow: info.workflow.unwrap_or_default(),
            dial: None,
        }
    }

    pub(super) async fn run(mut self) {
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

    pub(super) async fn next(&mut self) -> Option<Cmd> {
        if let Some(command) = self.deferred.pop_front() {
            return Some(command);
        }
        if let Some(next) = self.followups.pop_front() {
            announce(&self.ui, &self.steering, &self.followups);
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

    pub(super) fn tick(&mut self) {
        for event in self.loop_.child_events() {
            drop(self.ui.send(Msg::Event(event)));
        }
        self.loop_.bus_heartbeat("idle");
        let peers: Vec<_> = self
            .loop_
            .bus_agents()
            .into_iter()
            .filter(|agent| agent.id != self.loop_.session_id())
            .collect();
        let ids: Vec<String> = peers.iter().map(|agent| agent.id.clone()).collect();
        if self.last_peers.as_ref() != Some(&ids) {
            self.last_peers = Some(ids);
            drop(self.ui.send(Msg::Peers(peers)));
        }
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

    pub(super) fn queue_mail(&mut self) {
        if self.auto_reply {
            self.pending_mail = true;
        }
    }

    pub(super) fn set_bus(&mut self, on: bool) {
        self.auto_reply = on;
        if !on {
            self.pending_mail = false;
        }
    }

    pub(super) fn system(&self, text: &str) {
        drop(self.ui.send(Msg::System(text.to_string())));
    }

    pub(super) fn export(&self, target: Option<String>) {
        export_session(
            self.loop_.session_path(),
            &self.loop_.session_id(),
            target,
            &self.ui,
        );
    }

    pub(super) fn fail(&self, text: &str) {
        drop(self.ui.send(Msg::Failed(text.to_string())));
    }
}
