use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures::stream::{FuturesUnordered, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;

use super::*;
use crate::child::{Access, ChildRecord, ChildResult, ChildState, bounded};
use crate::durable::id::SessionId;
use crate::durable::log::{self, State, Usage as Meter};
use crate::tools::{ToolOutput, tool};
use crate::worktree::{self, Tree};

const LAST_CHARS: usize = 120;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    #[serde(default)]
    assignment: String,
    #[serde(default)]
    access: Access,
    #[serde(default)]
    background: bool,
    #[serde(default)]
    child: Option<String>,
    #[serde(default)]
    kill: bool,
    #[serde(skip)]
    call: String,
}

pub(super) struct Execution {
    pub(super) output: ToolOutput,
    pub(super) usage: Usage,
    pub(super) millis: u64,
}

pub(super) struct Spawned {
    record: ChildRecord,
    path: PathBuf,
    handle: JoinHandle<Done>,
    started: Instant,
    last: String,
    reserved_tokens: u64,
}

impl Spawned {
    pub(super) fn abort(&self) {
        self.handle.abort();
    }
}

struct Run<P: Provider> {
    child: Loop<P>,
    record: ChildRecord,
    guard: Guard,
    assignment: String,
    deadline: Option<Duration>,
    started: Instant,
}

struct Guard {
    tree: Option<Tree>,
}

impl Guard {
    fn disarm(&mut self) -> Option<Tree> {
        self.tree.take()
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(tree) = self.tree.take()
            && worktree::dirty(&tree.path).is_ok_and(|dirty| !dirty)
        {
            drop(worktree::remove_child(&tree));
        }
    }
}

pub(super) struct Done {
    record: ChildRecord,
    tree: Option<Tree>,
    paths: Vec<String>,
    usage: Usage,
    billed: u64,
    millis: u64,
    settled: bool,
}

pub(super) fn definition() -> ToolDefinition {
    tool(
        "task",
        "Delegate one assignment to a child. Same tools except task. Several calls in one response run together. background true returns an id; pass child to poll, kill true to stop. implement edits this workspace; research is read-only.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "assignment": {"type": "string"},
                "access": {"type": "string", "enum": ["implement", "research"], "default": "implement"},
                "background": {"type": "boolean"},
                "child": {"type": "string"},
                "kill": {"type": "boolean"}
            },
            "additionalProperties": false
        }),
    )
}

impl<P: Provider> Loop<P> {
    pub(super) fn task_available(&self) -> bool {
        self.tasks && self.depth == 0 && self.seat(self.round_role()).0.supports_children()
    }

    pub(super) fn child_budget_reserved(&self) -> u64 {
        self.spawned
            .values()
            .map(|spawned| spawned.reserved_tokens)
            .fold(0, u64::saturating_add)
    }

    pub(super) async fn task_execute(
        &mut self,
        call: &ToolCall,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Result<Execution> {
        self.tasks_execute(std::slice::from_ref(call), events, emit)
            .await
            .pop()
            .ok_or_else(|| Error::Config("task execution returned no result".to_string()))?
    }

    pub(super) async fn tasks_execute(
        &mut self,
        calls: &[ToolCall],
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Vec<Result<Execution>> {
        if calls.is_empty() {
            return Vec::new();
        }
        if self.depth != 0 {
            return rejected(calls.len(), "child sessions cannot delegate another task");
        }
        let mut args = Vec::with_capacity(calls.len());
        for call in calls {
            match task_args(call) {
                Ok(mut value) => {
                    value.call.clone_from(&call.id);
                    args.push(value);
                }
                Err(error) => return rejected(calls.len(), &error.to_string()),
            }
        }
        if args
            .iter()
            .all(|args| args.child.is_none() && !args.background)
        {
            return self.task_batch(args, events, emit).await;
        }
        let mut background = Vec::new();
        let mut results: Vec<Option<Result<Execution>>> =
            std::iter::repeat_with(|| None).take(args.len()).collect();
        for (index, args) in args.into_iter().enumerate() {
            if args.child.is_none() && args.background {
                background.push((index, args));
                continue;
            }
            let result = match args.child.clone() {
                Some(id) => self.task_poll(&id, args.kill, events, emit).await,
                None => self
                    .task_batch(vec![args], events, emit)
                    .await
                    .pop()
                    .unwrap_or_else(|| {
                        Err(Error::Config(
                            "task execution returned no result".to_string(),
                        ))
                    }),
            };
            results[index] = Some(result);
        }
        if !background.is_empty() {
            let budget = match self.task_budget(background.len()) {
                Ok(budget) => budget,
                Err(error) => {
                    let message = error.to_string();
                    for (index, _) in background {
                        results[index] = Some(Err(Error::Config(message.clone())));
                    }
                    return results
                        .into_iter()
                        .map(|result| {
                            result.unwrap_or_else(|| {
                                Err(Error::Config("task result was not recorded".to_string()))
                            })
                        })
                        .collect();
                }
            };
            for (index, args) in background {
                results[index] = Some(self.task_spawn(args, &budget));
            }
        }
        results
            .into_iter()
            .map(|result| {
                result.unwrap_or_else(|| {
                    Err(Error::Config("task result was not recorded".to_string()))
                })
            })
            .collect()
    }

    async fn task_batch(
        &mut self,
        args: Vec<Args>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Vec<Result<Execution>> {
        let count = args.len();
        let budget = match self.task_budget(count) {
            Ok(budget) => budget,
            Err(error) => return rejected(count, &error.to_string()),
        };
        let mut runs = Vec::with_capacity(count);
        for args in args {
            let access = args.access;
            runs.push((access, self.task_start(args, &budget, None)));
        }
        let sender = self.mailbox.0.clone();
        let mut runs: FuturesUnordered<_> = runs
            .into_iter()
            .enumerate()
            .map(|(index, (access, run))| {
                let sender = sender.clone();
                async move {
                    let done = match run {
                        Ok(run) => Ok(run.execute(sender).await),
                        Err(error) => Err(error),
                    };
                    (index, access, done)
                }
            })
            .collect();
        let mut finished = Vec::with_capacity(count);
        while finished.len() < count {
            tokio::select! {
                Some((index, access, done)) = runs.next() => {
                    let done = match done {
                        Ok(mut done) if access == Access::Research => {
                            self.task_record(&mut done).map(|()| done)
                        }
                        other => other,
                    };
                    finished.push((index, access, done));
                }
                Some(event) = self.mailbox.1.recv() => {
                    self.saw(&event);
                    emit(&event);
                    events.push(event);
                }
            }
        }
        finished.sort_by_key(|(index, _, _)| *index);
        let mut done: Vec<(Access, Result<Done>)> = finished
            .into_iter()
            .map(|(_, access, done)| (access, done))
            .collect();
        self.drain(events, emit);
        self.task_reconcile(&mut done);
        let mut results = Vec::with_capacity(done.len());
        for (_, done) in done {
            results.push(done.and_then(|done| self.task_settle(done)));
        }
        results
    }

    fn task_record(&mut self, done: &mut Done) -> Result<()> {
        self.session
            .finish_child(done.record.clone())
            .map_err(super::session_err)?;
        self.session
            .charge(
                super::meter(&done.usage, 0),
                &format!("child {}", done.record.id),
                &done.record.model,
                true,
            )
            .map_err(super::session_err)?;
        let reported =
            u64::from(done.usage.prompt_tokens) + u64::from(done.usage.completion_tokens);
        if done.billed > reported {
            self.session
                .charge(
                    Meter {
                        input: done.billed - reported,
                        ..Meter::default()
                    },
                    &format!("child {} unconfirmed usage", done.record.id),
                    &done.record.model,
                    false,
                )
                .map_err(super::session_err)?;
        }
        self.spent_tokens = self.session.replay().spent_tokens;
        done.settled = true;
        Ok(())
    }

    fn task_spawn(&mut self, args: Args, budget: &Budget) -> Result<Execution> {
        let run = self.task_start(args, budget, None)?;
        let record = run.record.clone();
        let path = run.child.session.path().to_path_buf();
        let id = record.id.to_string();
        let handle = tokio::spawn(run.execute(self.mailbox.0.clone()));
        self.spawned.insert(
            id.clone(),
            Spawned {
                record: record.clone(),
                path,
                handle,
                started: Instant::now(),
                last: String::new(),
                reserved_tokens: budget.max_tokens.unwrap_or_default(),
            },
        );
        Ok(Execution {
            output: ToolOutput {
                text: format!(
                    "started child {id}\naccess: {}\nassignment: {}\nstate: running\npoll with task {{\"child\": \"{id}\"}}; stop with kill true",
                    access_name(record.access),
                    bounded(&record.assignment, LAST_CHARS)
                ),
                success: true,
                mutated: false,
                grant: None,
            },
            usage: Usage::default(),
            millis: 0,
        })
    }

    async fn task_poll(
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
                        bounded(&spawned.record.assignment, LAST_CHARS),
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

    async fn finished(&self, spawned: Spawned) -> Done {
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

    pub(super) async fn await_children(
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
                        text: format!("child {} finished: {}", short(&id), execution.output.text),
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
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        Ok(self
            .spawned
            .values()
            .map(|spawned| {
                format!(
                    "child {} `{}`",
                    spawned.record.id,
                    bounded(&spawned.record.assignment, 60)
                )
            })
            .collect())
    }

    pub(super) async fn kill_spawned(
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

    pub(super) fn drain(&mut self, events: &mut Vec<Event>, emit: &mut impl FnMut(&Event)) {
        for event in self.child_events() {
            emit(&event);
            events.push(event);
        }
    }

    pub fn child_events(&mut self) -> Vec<Event> {
        let mut drained = Vec::new();
        while let Ok(event) = self.mailbox.1.try_recv() {
            self.saw(&event);
            drained.push(event);
        }
        drained
    }

    fn saw(&mut self, event: &Event) {
        let Event::Child { id, event } = event else {
            return;
        };
        let Some(spawned) = self.spawned.get_mut(id) else {
            return;
        };
        let last = match &**event {
            Event::ToolCall {
                name, arguments, ..
            } => format!("{name} {}", bounded(arguments, LAST_CHARS)),
            Event::ToolResult {
                success: false,
                output,
                ..
            } => format!("failed: {}", bounded(output, LAST_CHARS)),
            Event::Assistant { text } | Event::Note { text } => bounded(text, LAST_CHARS),
            Event::Aborted { reason } => format!("aborted: {}", bounded(reason, LAST_CHARS)),
            _ => return,
        };
        spawned.last = last;
    }

    fn task_start(&mut self, args: Args, budget: &Budget, tree: Option<Tree>) -> Result<Run<P>> {
        let mut guard = Guard { tree };
        let result = self.task_start_in(args, budget, guard.tree.clone());
        if result.is_ok() {
            drop(guard.disarm());
        }
        result
    }

    fn task_start_in(&mut self, args: Args, budget: &Budget, tree: Option<Tree>) -> Result<Run<P>> {
        let role = self.round_role();
        let provider = self.seat(role).0.child().ok_or_else(|| {
            Error::Denied("the active provider cannot create children".to_string())
        })?;
        let model = self.seat(role).1.to_string();
        let home = log::home_of(self.session.path())
            .ok_or_else(|| Error::Config("the parent session has no durable home".to_string()))?;
        let workspace = tree
            .as_ref()
            .map_or_else(|| self.workspace.clone(), |tree| tree.path.clone());
        let driver = SessionDriver::open_child(
            home,
            &workspace,
            &model,
            &self.system_prompt,
            self.session.id(),
        )
        .map_err(super::session_err)?;
        let instruction = match args.access {
            Access::Implement => {
                "Edit this workspace. You may build, test, and lint here. Background commands stay with the parent."
            }
            Access::Research => "This assignment is read-only. Do not change the workspace.",
        };
        let mut child = Self::with_parts(
            provider,
            model.clone(),
            &workspace,
            format!(
                "{}\n\nHarness: child. Finish only the assigned task. {instruction}",
                self.system_prompt
            ),
            driver,
            Policy::builtin(),
            None,
        )?;
        child.tools = self.tools.child_at(&workspace, &child.session_id())?;
        if args.access == Access::Research {
            child.tools.set_read_only(true);
        }
        child.depth = 1;
        child.mode = self.mode;
        child.context_window = self.context_window;
        child.context_sample = self.context_sample;
        child.effort = self.effort_for(role);
        child.price = self.price_for(role);
        child = child.with_budget(budget.clone())?;

        let record = ChildRecord {
            version: 1,
            id: child.session.id(),
            parent: self.session.id(),
            assignment: args.assignment.clone(),
            model: model.clone(),
            workspace,
            depth: 1,
            access: args.access,
            call: Some(args.call.clone()),
            state: ChildState::Running,
        };
        self.session
            .start_child(record.clone())
            .map_err(super::session_err)?;
        Ok(Run {
            child,
            record,
            guard: Guard { tree },
            assignment: args.assignment,
            deadline: budget.max_secs.map(Duration::from_secs),
            started: Instant::now(),
        })
    }

    fn task_reconcile(&self, done: &mut [(Access, Result<Done>)]) {
        let failed = done.iter().any(|(access, done)| {
            *access == Access::Implement
                && match done {
                    Ok(done) => !matches!(done.record.state, ChildState::Completed(_)),
                    Err(_) => true,
                }
        });
        if failed {
            for (access, done) in done {
                if *access == Access::Implement
                    && let Ok(done) = done
                    && matches!(done.record.state, ChildState::Completed(_))
                {
                    fail_record(
                        &mut done.record,
                        "writable child changes were not reconciled because another implement child did not complete; the worktree was kept",
                    );
                }
            }
            return;
        }
        let children = done
            .iter()
            .filter_map(|(access, done)| {
                if *access != Access::Implement {
                    return None;
                }
                let done = done.as_ref().ok()?;
                Some((done.tree.clone()?, done.paths.clone()))
            })
            .collect::<Vec<_>>();
        if children.is_empty() {
            return;
        }
        match worktree::reconcile_children(&self.workspace, &children) {
            Ok(warnings) if warnings.is_empty() => {}
            Ok(warnings) => {
                let warning = warnings.join("; ");
                for (access, done) in done {
                    if *access == Access::Implement
                        && let Ok(done) = done
                    {
                        note_record(&mut done.record, &warning);
                    }
                }
            }
            Err(error) => {
                let diagnostic = error.to_string();
                for (access, done) in done {
                    if *access == Access::Implement
                        && let Ok(done) = done
                    {
                        fail_record(&mut done.record, &diagnostic);
                    }
                }
            }
        }
    }

    fn task_settle(&mut self, mut done: Done) -> Result<Execution> {
        if !done.settled {
            self.task_record(&mut done)?;
        }
        let record = done.record;
        let text = serde_json::to_string(&record)
            .map_err(|error| Error::Config(format!("cannot encode child receipt: {error}")))?;
        let success = matches!(record.state, ChildState::Completed(_));
        let mutated = record_paths(&record) > 0;
        Ok(Execution {
            output: ToolOutput {
                text,
                success,
                mutated,
                grant: None,
            },
            usage: done.usage,
            millis: done.millis,
        })
    }

    fn task_budget(&self, children: usize) -> Result<Budget> {
        let children = u64::try_from(children)
            .map_err(|error| Error::Config(format!("task batch size is invalid: {error}")))?;
        if children == 0 {
            return Err(Error::Config("task batch is empty".to_string()));
        }
        let tokens = self.budget.max_tokens.map(|limit| {
            limit
                .saturating_sub(self.spent_tokens)
                .saturating_sub(self.child_budget_reserved())
                / children
        });
        let secs = self
            .budget
            .max_secs
            .map(|limit| limit.saturating_sub(self.elapsed_secs()));
        if tokens == Some(0) || secs == Some(0) {
            return Err(Error::Denied(
                "the parent budget has no capacity for a child".to_string(),
            ));
        }
        Ok(Budget {
            max_tokens: tokens,
            max_secs: secs,
        })
    }

    fn result(
        &self,
        text: &str,
        rounds: usize,
        usage: Usage,
        diagnostic: Option<&str>,
        access: Access,
    ) -> ChildResult {
        let paths = match access {
            Access::Implement => Self::touched_at(self.session.path()),
            Access::Research => Vec::new(),
        };
        let total = paths.len();
        let replay = self.session.replay();
        ChildResult {
            text: text.to_string(),
            changed_paths: paths,
            changed_paths_total: total,
            rounds,
            usage,
            billed_tokens: replay.spent_tokens,
            elapsed_millis: replay.elapsed_millis,
            diagnostic: diagnostic.map(str::to_string),
        }
    }
}

impl<P: Provider> Run<P> {
    async fn execute(mut self, sender: UnboundedSender<Event>) -> Done {
        let id = self.record.id.to_string();
        let emit = move |event: &Event| {
            drop(sender.send(Event::Child {
                id: id.clone(),
                event: Box::new(event.clone()),
            }));
        };
        let outcome = match self.deadline {
            Some(deadline) => {
                tokio::time::timeout(
                    deadline,
                    Box::pin(self.child.turn_with(self.assignment.as_str(), emit)),
                )
                .await
            }
            None => Ok(self.child.turn_with(self.assignment.as_str(), emit).await),
        };
        let access = self.record.access;
        let (state, mut usage) = match outcome {
            Ok(Ok(turn)) => {
                let state = if turn.aborted {
                    ChildState::Aborted(self.child.result(
                        &turn.text,
                        turn.rounds,
                        turn.usage.clone(),
                        Some("the child reached its round or resource budget"),
                        access,
                    ))
                } else {
                    ChildState::Completed(self.child.result(
                        &turn.text,
                        turn.rounds,
                        turn.usage.clone(),
                        None,
                        access,
                    ))
                };
                (state, turn.usage)
            }
            Ok(Err(error)) => {
                let diagnostic = match self.child.abort_note(&error.to_string()) {
                    Ok(()) => error.to_string(),
                    Err(abort) => format!("{error}; cannot close child turn: {abort}"),
                };
                (
                    ChildState::Failed(self.child.result(
                        "",
                        0,
                        Usage::default(),
                        Some(&diagnostic),
                        access,
                    )),
                    Usage::default(),
                )
            }
            Err(_) => {
                let reason = self.deadline.map_or_else(
                    || "child deadline reached".to_string(),
                    |deadline| format!("child deadline reached after {}s", deadline.as_secs()),
                );
                let diagnostic = match self.child.abort_note(&reason) {
                    Ok(()) => reason,
                    Err(error) => format!("{reason}; cannot close child turn: {error}"),
                };
                (
                    ChildState::Aborted(self.child.result(
                        "",
                        0,
                        Usage::default(),
                        Some(&diagnostic),
                        access,
                    )),
                    Usage::default(),
                )
            }
        };
        self.record.state = state;
        if !matches!(self.record.state, ChildState::Completed(_)) {
            match accounting_at(self.child.session.path()) {
                Ok((_, reported)) => {
                    usage = reported;
                    if let Some(result) = result_mut(&mut self.record.state) {
                        result.usage.clone_from(&usage);
                    }
                }
                Err(error) => fail_record(&mut self.record, &error.to_string()),
            }
        }
        let paths = match access {
            Access::Implement => Loop::<P>::touched_at(self.child.session.path()),
            Access::Research => Vec::new(),
        };
        Done {
            record: self.record,
            tree: self.guard.disarm(),
            paths,
            usage,
            billed: self.child.session.replay().spent_tokens,
            settled: false,
            millis: super::elapsed_millis(self.started),
        }
    }
}

async fn killed(spawned: Spawned, parent: &Path, reason: &str) -> Result<Done> {
    spawned.handle.abort();
    let mut record = spawned.record;
    let mut diagnostic = reason.to_string();
    if let Ok(done) = spawned.handle.await {
        return Ok(done);
    }
    if record.access == Access::Implement && record.workspace != parent && record.workspace.exists()
    {
        diagnostic.push_str(&format!(
            "; the worktree at {} holds changes and was kept",
            record.workspace.display()
        ));
    }
    let (billed, usage) = accounting_at(&spawned.path)?;
    let mut result = empty(Some(&diagnostic));
    result.billed_tokens = billed;
    result.usage = usage.clone();
    record.state = ChildState::Aborted(result);
    Ok(Done {
        record,
        tree: None,
        paths: Vec::new(),
        usage,
        billed,
        settled: false,
        millis: super::elapsed_millis(spawned.started),
    })
}

fn accounting_at(path: &Path) -> Result<(u64, Usage)> {
    let mut state = State::default();
    let mut usage = Usage::default();
    let lines = log::read(path)
        .map_err(|error| Error::Config(format!("cannot read child session log: {error}")))?;
    for line in lines {
        match &line {
            log::Line::Assistant { usage: meter, .. }
            | log::Line::Charge {
                usage: meter,
                reported: true,
                ..
            } => {
                usage.prompt_tokens = usage
                    .prompt_tokens
                    .saturating_add(u32::try_from(meter.input).unwrap_or(u32::MAX));
                usage.completion_tokens = usage
                    .completion_tokens
                    .saturating_add(u32::try_from(meter.output).unwrap_or(u32::MAX));
                usage.cached_tokens = usage
                    .cached_tokens
                    .saturating_add(u32::try_from(meter.cached).unwrap_or(u32::MAX));
            }
            _ => {}
        }
        state.apply(line);
    }
    Ok((state.spent, usage))
}

fn empty(diagnostic: Option<&str>) -> ChildResult {
    ChildResult {
        text: String::new(),
        changed_paths: Vec::new(),
        changed_paths_total: 0,
        rounds: 0,
        usage: Usage::default(),
        billed_tokens: 0,
        elapsed_millis: 0,
        diagnostic: diagnostic.map(str::to_string),
    }
}

fn short(id: &str) -> String {
    id.chars().take(8).collect()
}

fn access_name(access: Access) -> &'static str {
    match access {
        Access::Implement => "implement",
        Access::Research => "research",
    }
}

fn note_record(record: &mut ChildRecord, diagnostic: &str) {
    if let Some(result) = result_mut(&mut record.state) {
        result.diagnostic = Some(diagnostic.to_string());
    }
}

fn fail_record(record: &mut ChildRecord, diagnostic: &str) {
    let state = std::mem::replace(&mut record.state, ChildState::Running);
    let mut result = match state {
        ChildState::Completed(result)
        | ChildState::Aborted(result)
        | ChildState::Failed(result)
        | ChildState::Interrupted(result) => result,
        ChildState::Running => return,
    };
    result.diagnostic = Some(diagnostic.to_string());
    record.state = ChildState::Failed(result);
}

fn result_mut(state: &mut ChildState) -> Option<&mut ChildResult> {
    match state {
        ChildState::Running => None,
        ChildState::Completed(result)
        | ChildState::Aborted(result)
        | ChildState::Failed(result)
        | ChildState::Interrupted(result) => Some(result),
    }
}

fn task_args(call: &ToolCall) -> Result<Args> {
    let mut args: Args = serde_json::from_str(&call.function.arguments)
        .map_err(|error| Error::Tool(format!("task arguments are invalid: {error}")))?;
    if let Some(id) = &args.child {
        if id.trim().is_empty() {
            return Err(Error::Tool("task child id is empty".to_string()));
        }
        if !args.assignment.trim().is_empty() {
            return Err(Error::Tool(
                "task takes either child or assignment, not both".to_string(),
            ));
        }
        return Ok(args);
    }
    if args.kill {
        return Err(Error::Tool("task kill requires child".to_string()));
    }
    let assignment = args.assignment.trim();
    if assignment.is_empty() {
        return Err(Error::Tool("task requires assignment".to_string()));
    }
    args.assignment = assignment.to_string();
    Ok(args)
}

fn rejected(count: usize, message: &str) -> Vec<Result<Execution>> {
    (0..count)
        .map(|_| Err(Error::Denied(message.to_string())))
        .collect()
}

fn record_paths(record: &ChildRecord) -> usize {
    match &record.state {
        ChildState::Running => 0,
        ChildState::Completed(result)
        | ChildState::Aborted(result)
        | ChildState::Failed(result)
        | ChildState::Interrupted(result) => result.changed_paths_total,
    }
}
