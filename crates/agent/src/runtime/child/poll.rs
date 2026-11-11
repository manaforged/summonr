use super::record::*;
use super::*;
use tokio::task::JoinError;

const WAITING_CHARS: usize = 60;

enum Wake {
    Event(Event),
    Joined(String, Box<std::result::Result<Done, JoinError>>),
}

pub(in crate::runtime) enum Kill<'a> {
    TurnEnded,
    Because(&'a str),
}

impl<P: Provider> Loop<P> {
    pub(super) async fn task_poll(
        &mut self,
        id: &str,
        kill: bool,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Execution> {
        self.drain(emit);
        if !self.spawned.contains_key(id) {
            let settled = id
                .parse::<SessionId>()
                .ok()
                .and_then(|id| self.session.child(id))
                .ok_or_else(|| Error::Tool(format!("unknown child `{id}`")))?;
            return Ok(Execution {
                output: ToolOutput::ok(serde_json::to_string(&settled)?)
                    .with_success(kill || matches!(settled.state, ChildState::Completed(_))),
                usage: Usage::default(),
                millis: 0,
            });
        }
        if kill {
            let Some(spawned) = self.spawned.remove(id) else {
                return Err(Error::Tool(format!("unknown child `{id}`")));
            };
            let done = killed(spawned, &self.workspace, "killed by the parent").await?;
            let mut execution = self.task_settle(done)?;
            execution.output = execution.output.with_success(true);
            return Ok(execution);
        }
        let finished = self
            .spawned
            .get(id)
            .is_some_and(|spawned| spawned.handle.is_finished());
        if !finished {
            let Some(spawned) = self.spawned.get(id) else {
                return Err(Error::Tool(format!("unknown child `{id}`")));
            };
            let state = spawned.state.borrow().clone();
            if state.terminal() {
                let success = matches!(state, ChildState::Completed(_));
                let record = ChildRecord {
                    state,
                    ..spawned.record.clone()
                };
                return Ok(Execution {
                    output: ToolOutput::ok(format!(
                        "child {id}\nstate: idle\n{}",
                        serde_json::to_string(&record)?
                    ))
                    .with_success(success),
                    usage: Usage::default(),
                    millis: 0,
                });
            }
            let tool = if spawned.tool.is_empty() {
                "idle"
            } else {
                spawned.tool.as_str()
            };
            let last = if spawned.last.is_empty() {
                "starting"
            } else {
                spawned.last.as_str()
            };
            return Ok(Execution {
                output: ToolOutput::ok(format!(
                    "child {id}\nstate: running\n{}\nelapsed: {}s\ntool: {tool}\nassignment: {}\nlast: {last}",
                    running_place(&spawned.record.workspace),
                    spawned.started.elapsed().as_secs(),
                    ellipsize(&spawned.record.assignment, LAST_CHARS),
                )),
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
                record.state =
                    ChildState::Failed(ChildResult::with_diagnostic(Some(&error.to_string())));
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
            self.drain(emit);
            self.settle_ready(events, emit, usage).await?;
            if !self
                .spawned
                .values()
                .any(|spawned| !spawned.state.borrow().terminal())
            {
                break;
            }
            if !announced {
                announced = true;
                self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: format!("waiting for {} subagent(s)", self.spawned.len()),
                    },
                )?;
            }
            match tokio::time::timeout_at(deadline, self.next_wake()).await {
                Ok(Wake::Event(event)) => {
                    self.saw(&event);
                    emit(&event);
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
            .filter(|spawned| !spawned.state.borrow().terminal())
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
        kill: Kill<'_>,
        usage: &mut Usage,
    ) -> Result<Vec<ChildRecord>> {
        let mut records = Vec::new();
        let ids: Vec<String> = self.spawned.keys().cloned().collect();
        let ending = matches!(kill, Kill::TurnEnded);
        let reason = match kill {
            Kill::TurnEnded => "the parent turn ended",
            Kill::Because(reason) => reason,
        };
        for spawned in self
            .spawned
            .values()
            .filter(|spawned| !(ending && spawned.stay))
        {
            spawned.handle.abort();
        }
        for id in &ids {
            let Some(spawned) = self.spawned.remove(id) else {
                continue;
            };
            if ending && spawned.stay {
                self.spawned.insert(id.clone(), spawned);
                continue;
            }
            let done = killed(spawned, &self.workspace, reason).await?;
            records.push(done.record.clone());
            usage.add(&self.task_settle(done)?.usage);
        }
        self.discard_events(&ids);
        Ok(records)
    }

    pub(crate) fn reap_finished(&mut self) -> Vec<String> {
        let ready: Vec<String> = self
            .spawned
            .iter()
            .filter(|(_, spawned)| spawned.handle.is_finished())
            .map(|(id, _)| id.clone())
            .collect();
        let mut notes = Vec::new();
        for id in ready {
            let Some(mut spawned) = self.spawned.remove(&id) else {
                continue;
            };
            let Some(joined) = poll_ready(&mut spawned.handle) else {
                self.spawned.insert(id, spawned);
                continue;
            };
            let done = self.joined(spawned, joined);
            let note = match self.task_settle(done) {
                Ok(execution) => format!(
                    "child {id} exited\n{}",
                    ellipsize(&execution.output.text, LAST_CHARS)
                ),
                Err(error) => format!("child {id} exited: {error}"),
            };
            notes.push(note);
        }
        notes
    }

    pub(in crate::runtime) async fn run_or_wake(
        &mut self,
        call: &ToolCall,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<(Result<ToolOutput>, Usage)> {
        let waits =
            crate::tools::Builtin::is(&call.function.name, crate::tools::Builtin::RunCommand)
                && serde_json::from_str::<serde_json::Value>(&call.function.arguments)
                    .is_ok_and(|args| args.get("job").is_some() && args.get("kill").is_none());
        let mut usage = Usage::default();
        if waits {
            self.settle_ready(events, emit, &mut usage).await?;
        }
        if !waits || self.spawned.is_empty() {
            let command =
                crate::tools::Builtin::is(&call.function.name, crate::tools::Builtin::RunCommand);
            let stops = command && (waits || self.input_interrupts());
            self.tools.yield_command(false);
            let input = self.input_wait();
            let run = self.tools.execute_outcome(call);
            tokio::pin!(input, run);
            let mut watching = command;
            let output = loop {
                tokio::select! {
                    biased;
                    result = &mut input, if watching => {
                        watching = false;
                        if stops {
                            result?;
                            break Err(Error::Tool(self.steered("interrupted by an incoming message; any completed changes remain")));
                        }
                        if result.is_ok() {
                            self.tools.yield_command(true);
                        }
                    },
                    output = &mut run => break output,
                }
            };
            return Ok((output, usage));
        }
        let ids: Vec<String> = self.spawned.keys().cloned().collect();
        let input = self.input_wait();
        let handles = self.spawned.values_mut().map(|spawned| &mut spawned.handle);
        let (index, joined) = tokio::select! {
            biased;
            result = input => {
                result?;
                return Ok((Err(Error::Tool(self.steered("job wait interrupted by an incoming message"))), usage));
            },
            (joined, index, _) = futures::future::select_all(handles) => (index, joined),
            output = self.tools.execute_outcome(call) => return Ok((output, usage)),
        };
        let id = &ids[index];
        self.announce_finished(id, joined, events, emit, &mut usage)?;
        Ok((
            Ok(ToolOutput::ok(format!(
                "returned early: background child {id} finished before this job did; its result is delivered; wait on the job again"
            ))),
            usage,
        ))
    }
}
