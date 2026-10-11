use super::record::*;
use super::*;
use tokio::task::JoinError;

const WAITING_CHARS: usize = 60;

enum Wake {
    Event(Event),
    Joined(String, Box<std::result::Result<Done, JoinError>>),
}

impl<P: Provider> Loop<P> {
    pub(super) async fn task_poll(
        &mut self,
        id: &str,
        kill: bool,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Execution> {
        self.drain(events, emit);
        if !self.spawned.contains_key(id) {
            let settled = id
                .parse::<SessionId>()
                .ok()
                .and_then(|id| self.session.child(id))
                .ok_or_else(|| Error::Tool(format!("unknown child `{id}`")))?;
            return Ok(Execution {
                output: ToolOutput {
                    text: serde_json::to_string(&settled)?,
                    success: matches!(settled.state, ChildState::Completed(_)),
                    mutated: false,
                    grant: None,
                    images: Vec::new(),
                    patch: None,
                },
                usage: Usage::default(),
                millis: 0,
            });
        }
        if kill {
            let Some(spawned) = self.spawned.remove(id) else {
                return Err(Error::Tool(format!("unknown child `{id}`")));
            };
            let done = killed(spawned, &self.workspace, "killed by the parent").await?;
            return self.task_settle(done);
        }
        let finished = self
            .spawned
            .get(id)
            .is_some_and(|spawned| spawned.handle.is_finished());
        if !finished {
            let Some(spawned) = self.spawned.get(id) else {
                return Err(Error::Tool(format!("unknown child `{id}`")));
            };
            return Ok(Execution {
                output: ToolOutput {
                    text: format!(
                        "child {id}\nstate: running\nelapsed: {}s\nassignment: {}\nlast: {}",
                        spawned.started.elapsed().as_secs(),
                        ellipsize(&spawned.record.assignment, LAST_CHARS),
                        if spawned.last.is_empty() {
                            "starting"
                        } else {
                            spawned.last.as_str()
                        }
                    ),
                    success: true,
                    mutated: false,
                    grant: None,
                    images: Vec::new(),
                    patch: None,
                },
                usage: Usage::default(),
                millis: 0,
            });
        }
        let Some(spawned) = self.spawned.remove(id) else {
            return Err(Error::Tool(format!("unknown child `{id}`")));
        };
        let done = self.finished(spawned).await;
        self.task_settle(done)
    }

    pub(super) async fn finished(&self, mut spawned: Spawned) -> Done {
        let joined = (&mut spawned.handle).await;
        self.joined(spawned, joined)
    }

    fn joined(&self, spawned: Spawned, joined: std::result::Result<Done, JoinError>) -> Done {
        let access = spawned.record.access;
        let done = joined.map_err(|error| Error::Tool(format!("child task failed: {error}")));
        let mut pair = [(access, done)];
        self.task_reconcile(&mut pair);
        let [(_, done)] = pair;
        match done {
            Ok(done) => done,
            Err(error) => {
                let mut record = spawned.record;
                record.state = ChildState::Failed(empty(Some(&error.to_string())));
                Done {
                    record,
                    tree: None,
                    paths: Vec::new(),
                    usage: Usage::default(),
                    billed: 0,
                    settled: false,
                    millis: super::elapsed_millis(spawned.started),
                }
            }
        }
    }

    pub(in crate::runtime) async fn await_children(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<Vec<String>> {
        let deadline = tokio::time::Instant::now() + crate::limits::limits().job_wait();
        let mut announced = false;
        loop {
            self.drain(events, emit);
            self.settle_ready(events, emit, usage).await?;
            if self.spawned.is_empty() {
                break;
            }
            if !announced {
                announced = true;
                self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: format!("waiting for {} background child(ren)", self.spawned.len()),
                    },
                )?;
            }
            match tokio::time::timeout_at(deadline, self.next_wake()).await {
                Ok(Wake::Event(event)) => {
                    self.saw(&event);
                    emit(&event);
                    events.push(event);
                }
                Ok(Wake::Joined(id, joined)) => {
                    self.announce_finished(&id, *joined, events, emit, usage)?;
                }
                Err(_) => break,
            }
        }
        Ok(self
            .spawned
            .values()
            .map(|spawned| {
                format!(
                    "child {} `{}`",
                    spawned.record.id,
                    ellipsize(&spawned.record.assignment, WAITING_CHARS)
                )
            })
            .collect())
    }

    async fn next_wake(&mut self) -> Wake {
        let ids: Vec<String> = self.spawned.keys().cloned().collect();
        let handles = self.spawned.values_mut().map(|spawned| &mut spawned.handle);
        let receiver = &mut self.events.1;
        tokio::select! {
            Some(event) = receiver.recv() => Wake::Event(event),
            (joined, index, _) = futures::future::select_all(handles) => {
                Wake::Joined(ids[index].clone(), Box::new(joined))
            }
        }
    }

    async fn settle_ready(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<()> {
        let ready: Vec<String> = self
            .spawned
            .iter()
            .filter(|(_, spawned)| spawned.handle.is_finished())
            .map(|(id, _)| id.clone())
            .collect();
        for id in ready {
            let Some(spawned) = self.spawned.get_mut(&id) else {
                continue;
            };
            let joined = (&mut spawned.handle).await;
            self.announce_finished(&id, joined, events, emit, usage)?;
        }
        Ok(())
    }

    fn announce_finished(
        &mut self,
        id: &str,
        joined: std::result::Result<Done, JoinError>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
        usage: &mut Usage,
    ) -> Result<()> {
        let Some(spawned) = self.spawned.remove(id) else {
            return Ok(());
        };
        let done = self.joined(spawned, joined);
        let execution = self.task_settle(done)?;
        usage.add(&execution.usage);
        self.push_event(
            events,
            emit,
            Event::Note {
                text: format!(
                    "child {} finished: {}",
                    crate::child::tag(id),
                    execution.output.text
                ),
            },
        )
    }

    pub(in crate::runtime) async fn kill_spawned(
        &mut self,
        reason: &str,
        usage: &mut Usage,
    ) -> Result<Vec<ChildRecord>> {
        let mut records = Vec::new();
        let ids: Vec<String> = self.spawned.keys().cloned().collect();
        for id in &ids {
            let Some(spawned) = self.spawned.remove(id) else {
                continue;
            };
            let done = killed(spawned, &self.workspace, reason).await?;
            records.push(done.record.clone());
            usage.add(&self.task_settle(done)?.usage);
        }
        self.discard_events(&ids);
        Ok(records)
    }

    pub(in crate::runtime) fn discard_events(&mut self, ids: &[String]) {
        let ids: std::collections::HashSet<&String> = ids.iter().collect();
        let mut kept = Vec::new();
        while let Ok(event) = self.events.1.try_recv() {
            if !matches!(&event, Event::Child { id, .. } if ids.contains(id)) {
                kept.push(event);
            }
        }
        for event in kept {
            drop(self.events.0.send(event));
        }
    }

    pub(in crate::runtime) fn drain(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) {
        for event in self.child_events() {
            emit(&event);
            events.push(event);
        }
    }

    pub(crate) fn child_events(&mut self) -> Vec<Event> {
        let mut drained = Vec::new();
        while let Ok(event) = self.events.1.try_recv() {
            self.saw(&event);
            drained.push(event);
        }
        drained
    }

    pub(super) fn saw(&mut self, event: &Event) {
        let Event::Child { id, event } = event else {
            return;
        };
        let Some(spawned) = self.spawned.get_mut(id) else {
            return;
        };
        let last = match &**event {
            Event::ToolCall {
                name, arguments, ..
            } => format!("{name} {}", ellipsize(arguments, LAST_CHARS)),
            Event::ToolResult {
                success: false,
                output,
                ..
            } => format!("failed: {}", ellipsize(output, LAST_CHARS)),
            Event::Assistant { text } | Event::Note { text } => ellipsize(text, LAST_CHARS),
            Event::Aborted { reason, .. } => format!("aborted: {}", ellipsize(reason, LAST_CHARS)),
            _ => return,
        };
        spawned.last = last;
    }
}
