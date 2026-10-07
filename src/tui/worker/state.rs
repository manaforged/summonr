use super::export::export_session;
use super::*;

impl Worker {
    pub(super) fn new(
        mut loop_: Loop<Client>,
        cmd: UnboundedReceiver<Cmd>,
        ui: UnboundedSender<Msg>,
        info: Info,
    ) -> Self {
        let steering = Arc::new(crate::runtime::Steering::default());
        loop_.steer_with(steering.clone());
        let home = info
            .config
            .as_deref()
            .and_then(std::path::Path::parent)
            .map(std::path::Path::to_path_buf);
        let reader = Reader::new(
            loop_.workspace().to_path_buf(),
            loop_.mcp_board(),
            ui.clone(),
        );
        Self {
            home,
            reader,
            loop_,
            cmd,
            ui,
            endpoint: info.endpoint,
            relay: info.relay,
            auto_reply: info.prefs.mail,
            pending_mail: false,
            last_auto: Instant::now(),
            steering,
            deferred: VecDeque::new(),
            followups: VecDeque::new(),
            carried: 0,
            last_turn: None,
            mail_error: None,
            last_diff: None,
            last_peers: None,
            workflow: info.workflow.unwrap_or_default(),
            dial: None,
            levels: Vec::new(),
        }
    }

    pub(super) async fn run(mut self) {
        self.replay();
        drop(self.ui.send(Msg::Session(self.loop_.session_id())));
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
        if self.carried > 0
            && let Some(next) = self.followups.pop_front()
        {
            self.carried -= 1;
            announce(&self.ui, &self.steering, &self.followups);
            return Some(Cmd::Submit(next));
        }
        self.carried = 0;
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

    fn peers(&mut self) {
        let peers: Vec<_> = self
            .loop_
            .peers()
            .into_iter()
            .filter(|agent| agent.id != self.loop_.session_id())
            .collect();
        let last = self.last_peers.as_deref().unwrap_or(&[]);
        let same = last.len() == peers.len()
            && last.iter().zip(&peers).all(|(a, b)| {
                a.id == b.id
                    && a.name == b.name
                    && a.kind == b.kind
                    && a.model == b.model
                    && a.cwd == b.cwd
                    && a.pid == b.pid
                    && a.status == b.status
            });
        if !same {
            self.last_peers = Some(peers.clone());
            drop(self.ui.send(Msg::Peers(peers)));
        }
    }

    pub(super) fn tick(&mut self) {
        for event in self.loop_.child_events() {
            drop(self.ui.send(Msg::Event(event)));
        }
        self.loop_.mail_heartbeat(crate::mail::Presence::Idle);
        self.peers();
        let mail = match self.loop_.mail_poll() {
            Ok(mail) => {
                if self.mail_error.take().is_some() {
                    drop(self.ui.send(Msg::System("mail recovered".to_string())));
                }
                mail
            }
            Err(error) => {
                let text = format!("mail poll failed: {error}");
                if self.mail_error.as_ref() != Some(&text) {
                    drop(self.ui.send(Msg::System(text.clone())));
                    self.mail_error = Some(text);
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
                    from: message.sender().to_string(),
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

    pub(super) fn set_mail_reply(&mut self, on: bool) {
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
