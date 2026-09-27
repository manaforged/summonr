use super::record::*;
use super::*;

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

    pub(super) async fn finished(&self, spawned: Spawned) -> Done {
        let access = spawned.record.access;
        let done = match spawned.handle.await {
            Ok(done) => Ok(done),
            Err(error) => Err(Error::Tool(format!("child task failed: {error}"))),
        };
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
        let started = Instant::now();
        let mut announced = false;
        loop {
            self.drain(events, emit);
            let finished: Vec<String> = self
                .spawned
                .iter()
                .filter(|(_, spawned)| spawned.handle.is_finished())
                .map(|(id, _)| id.clone())
                .collect();
            for id in finished {
                let Some(spawned) = self.spawned.remove(&id) else {
                    continue;
                };
                let done = self.finished(spawned).await;
                let execution = self.task_settle(done)?;
                usage.add(&execution.usage);
                self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: format!(
                            "child {} finished: {}",
                            crate::child::tag(&id),
                            execution.output.text
                        ),
                    },
                )?;
            }
            if self.spawned.is_empty() || started.elapsed().as_secs() >= JOB_WAIT_SECS {
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
            tokio::time::sleep(crate::limits::CHILD_POLL).await;
        }
        Ok(self
            .spawned
            .values()
            .map(|spawned| {
                format!(
                    "child {} `{}`",
                    spawned.record.id,
                    ellipsize(&spawned.record.assignment, 60)
                )
            })
            .collect())
    }

    pub(in crate::runtime) async fn kill_spawned(
        &mut self,
        reason: &str,
        usage: &mut Usage,
    ) -> Result<Vec<ChildRecord>> {
        let mut records = Vec::new();
        let ids: Vec<String> = self.spawned.keys().cloned().collect();
        for id in ids {
            let Some(spawned) = self.spawned.remove(&id) else {
                continue;
            };
            let done = killed(spawned, &self.workspace, reason).await?;
            records.push(done.record.clone());
            usage.add(&self.task_settle(done)?.usage);
        }
        Ok(records)
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
            Event::Aborted { reason } => format!("aborted: {}", ellipsize(reason, LAST_CHARS)),
            _ => return,
        };
        spawned.last = last;
    }
}
