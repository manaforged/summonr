use super::export::export_session;
use super::*;
use crate::hub::Fact;

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
            info.trust,
        );
        let levels = match crate::endpoint::resolve(
            Some(&info.endpoint),
            Some(loop_.model()),
            crate::endpoint::Requirement::Config,
        ) {
            Ok(endpoint) => endpoint.reasoning.levels_for(loop_.model()),
            Err(error) => {
                drop(ui.send(Msg::Fact(Fact::Failed(error.to_string()))));
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
            mail_held: false,
            last_auto: Instant::now(),
            queue: super::steer::InputQueue::new(steering),
            last_turn: None,
            mail_error: None,
            announced: Vec::new(),
            titled: false,
            last_diff: None,
            last_peers: None,
            workflow: info.workflow.unwrap_or_default(),
            dial: None,
            levels,
        }
    }

    pub(super) async fn run(mut self, replace: bool) {
        self.replay(replace);
        drop(
            self.ui
                .send(Msg::Fact(Fact::Session(self.loop_.session_id()))),
        );
        if let Some(name) = self.loop_.workflow() {
            self.system(&format!("workflow {name} · {}", self.workflow.summary()));
        }
        self.send_workflow();
        self.send_settings();
        self.send_diff(false).await;
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
            self.show_delivered(&next);
            return Some(Cmd::Submit(next));
        }
        self.queue.carried = 0;
        if let Some(command) = self.queue.deferred.pop_front() {
            self.queue.announce(&self.ui);
            if let Cmd::Submit(input) = &command {
                self.show_delivered(input);
            }
            return Some(command);
        }
        if let Some(next) = self.queue.followups.pop_front() {
            self.queue.announce(&self.ui);
            self.show_delivered(&next);
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

    fn show_delivered(&mut self, input: &crate::provider::UserInput) {
        if self.queue.take_recorded(&input.text) {
            return;
        }
        drop(
            self.ui
                .send(Msg::Intent(crate::hub::Intent::User(input.text.clone()))),
        );
        for image in &input.images {
            drop(
                self.ui
                    .send(Msg::Intent(crate::hub::Intent::Image(image.clone()))),
            );
        }
    }

    pub(super) fn peers(&mut self, peers: &[crate::mail::Agent]) {
        publish_peers(
            peers,
            &self.loop_.session_id(),
            &mut self.last_peers,
            &self.ui,
        );
    }

    pub(super) fn tick(&mut self) {
        for event in self.loop_.child_events() {
            drop(self.ui.send(Msg::Fact(Fact::Event(event))));
        }
        let reaped = self
            .loop_
            .reap_finished()
            .into_iter()
            .map(|note| (note, "child"));
        let jobs = self
            .loop_
            .finished_jobs()
            .into_iter()
            .map(|note| (note, "job"));
        for (note, kind) in reaped.chain(jobs).collect::<Vec<_>>() {
            self.system(&note);
            match crate::mail::Mailbox::wake(&self.loop_.session_id(), &note) {
                Ok(()) => self.announced.push(note),
                Err(error) => self.system(&format!("{kind} exit was not delivered: {error}")),
            }
        }
        self.loop_.mail_heartbeat(crate::mail::Presence::Idle);
        let mail = self.polled_mail();
        if mail.is_empty() {
            return;
        }
        if self.loop_.mail_wakes(&mail) {
            self.queue_mail();
        }
        let monitor = mail.iter().any(crate::mail::Mail::from_monitor);
        if monitor && !self.mail_held {
            self.pending_mail = true;
        }
        let shown: Vec<bool> = mail
            .iter()
            .map(|message| {
                let at = message
                    .from_monitor()
                    .then(|| self.announced.iter().position(|note| *note == message.text))
                    .flatten();
                at.map(|at| self.announced.swap_remove(at)).is_some()
            })
            .collect();
        if !self.pending_mail {
            self.show_mail(mail, &shown);
        }
    }

    fn show_mail(&self, mail: Vec<crate::mail::Mail>, shown: &[bool]) {
        for (message, shown) in mail.into_iter().zip(shown) {
            if *shown {
                continue;
            }
            drop(self.ui.send(Msg::Fact(Fact::Mail {
                from: message.sender().to_string(),
                text: message.text.clone(),
            })));
        }
    }

    fn polled_mail(&mut self) -> Vec<crate::mail::Mail> {
        match self.loop_.mail_poll() {
            Ok(mail) => {
                if self.mail_error.take().is_some() {
                    drop(
                        self.ui
                            .send(Msg::Fact(Fact::System("mail recovered".to_string()))),
                    );
                }
                mail
            }
            Err(error) => {
                let text = format!("mail poll failed: {error}");
                if self.mail_error.as_ref() != Some(&text) {
                    drop(self.ui.send(Msg::Fact(Fact::System(text.clone()))));
                    self.mail_error = Some(text);
                }
                Vec::new()
            }
        }
    }

    pub(super) fn queue_mail(&mut self) {
        if self.auto_reply && !self.mail_held {
            self.pending_mail = true;
        }
    }

    pub(super) fn system(&self, text: &str) {
        drop(self.ui.send(Msg::Fact(Fact::System(text.to_string()))));
    }

    pub(super) fn outcome(&self, text: &str) {
        drop(self.ui.send(Msg::Fact(Fact::Outcome(text.to_string()))));
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
        drop(self.ui.send(Msg::Fact(Fact::Failed(text.to_string()))));
    }
}

pub(super) fn set_mail_reply(auto_reply: &mut bool, pending_mail: &mut bool, on: bool) {
    *auto_reply = on;
    if !on {
        *pending_mail = false;
    }
}

pub(super) fn publish_peers(
    peers: &[crate::mail::Agent],
    session: &str,
    previous: &mut Option<Vec<crate::mail::Agent>>,
    ui: &UnboundedSender<Msg>,
) {
    let peers: Vec<_> = peers.iter().filter(|agent| agent.id != session).collect();
    let last = previous.as_deref().unwrap_or(&[]);
    let same = last.len() == peers.len()
        && last.iter().zip(peers.iter().copied()).all(|(a, b)| {
            a.id == b.id
                && a.parent == b.parent
                && a.name == b.name
                && a.kind == b.kind
                && a.model == b.model
                && a.cwd == b.cwd
                && a.pid == b.pid
                && a.status == b.status
                && a.tool == b.tool
        });
    if !same {
        let peers: Vec<_> = peers.into_iter().cloned().collect();
        *previous = Some(peers.clone());
        drop(ui.send(Msg::Fact(Fact::Peers(peers))));
    }
}
