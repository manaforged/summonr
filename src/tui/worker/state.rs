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
        let levels = match crate::endpoint::resolve(
            Some(&info.endpoint),
            Some(loop_.model()),
            crate::endpoint::Need::Config,
        ) {
            Ok(endpoint) => endpoint.reasoning.levels_for(loop_.model()),
            Err(error) => {
                drop(ui.send(Msg::Failed(error.to_string())));
                Vec::new()
            }
        };
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
            queue: super::steer::InputQueue::new(steering),
            last_turn: None,
            mail_error: None,
            last_diff: None,
            last_peers: None,
            workflow: info.workflow.unwrap_or_default(),
            dial: None,
            levels,
        }
    }

    pub(super) async fn run(mut self) {
        self.replay();
        drop(self.ui.send(Msg::Session(self.loop_.session_id())));
        if let Some(name) = self.loop_.workflow() {
            self.system(&format!("workflow {name} · {}", self.workflow.summary()));
        }
        self.send_workflow();
        self.send_settings();
        self.reader.watch();
        while let Some(command) = self.next().await {
            if !self.dispatch(command).await {
                break;
            }
        }
    }

    pub(super) async fn next(&mut self) -> Option<Cmd> {
        if self.queue.carried > 0
            && let Some(next) = self.queue.followups.pop_front()
        {
            self.queue.carried -= 1;
            self.queue.announce(&self.ui);
            return Some(Cmd::Submit(next));
        }
        self.queue.carried = 0;
        if let Some(command) = self.queue.deferred.pop_front() {
            self.queue.announce(&self.ui);
            return Some(command);
        }
        if let Some(next) = self.queue.followups.pop_front() {
            self.queue.announce(&self.ui);
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
        publish_peers(
            self.loop_.peers(),
            &self.loop_.session_id(),
            &mut self.last_peers,
            &self.ui,
        );
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

    pub(super) fn system(&self, text: &str) {
        drop(self.ui.send(Msg::System(text.to_string())));
    }

    pub(super) fn outcome(&self, text: &str) {
        drop(self.ui.send(Msg::Outcome(text.to_string())));
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

pub(super) fn set_mail_reply(auto_reply: &mut bool, pending_mail: &mut bool, on: bool) {
    *auto_reply = on;
    if !on {
        *pending_mail = false;
    }
}

pub(super) fn publish_peers(
    peers: Vec<crate::mail::Agent>,
    session: &str,
    previous: &mut Option<Vec<crate::mail::Agent>>,
    ui: &UnboundedSender<Msg>,
) {
    let peers: Vec<_> = peers
        .into_iter()
        .filter(|agent| agent.id != session)
        .collect();
    let last = previous.as_deref().unwrap_or(&[]);
    let same = last.len() == peers.len()
        && last.iter().zip(&peers).all(|(a, b)| {
            a.id == b.id
                && a.parent == b.parent
                && a.name == b.name
                && a.kind == b.kind
                && a.model == b.model
                && a.cwd == b.cwd
                && a.pid == b.pid
                && a.status == b.status
        });
    if !same {
        *previous = Some(peers.clone());
        drop(ui.send(Msg::Peers(peers)));
    }
}
