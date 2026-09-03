use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use futures::future::join_all;
use serde::Deserialize;
use tokio::sync::mpsc::UnboundedSender;
use tokio::task::JoinHandle;

use super::*;
use crate::child::{
    ASSIGNMENT_MAX_CHARS, Access, ChildRecord, ChildResult, ChildState, MAX_PARALLEL, MAX_ROUNDS,
    MAX_SECS, PATH_MAX_CHARS, PATHS_MAX, RESULT_MAX_CHARS, bounded,
};
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
}

pub(super) struct Execution {
    pub(super) output: ToolOutput,
    pub(super) usage: Usage,
    pub(super) millis: u64,
}

/// One background child: its running record, task handle, and last visible action.
pub(super) struct Spawned {
    record: ChildRecord,
    path: PathBuf,
    handle: JoinHandle<Done>,
    started: Instant,
    last: String,
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
    deadline: Duration,
    started: Instant,
}

/// Removes a clean child worktree when the run is dropped before it settles.
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
}

pub(super) fn definition() -> ToolDefinition {
    tool(
        "task",
        "Delegate one bounded assignment to a fresh child session with the same tools except task. Emit up to four task calls together; the batch returns when every child finishes. Set background true to return at once with a child id, then pass child to poll it or child plus kill true to stop it. Implement children edit isolated Git worktrees and may build and test there; research children are read-only. The parent reconciles changes, verifies once, and commits.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "assignment": {
                    "type": "string",
                    "maxLength": ASSIGNMENT_MAX_CHARS
                },
                "access": {
                    "type": "string",
                    "enum": ["implement", "research"],
                    "default": "implement",
                    "description": "Implement edits an isolated worktree and can run commands there. Research is read-only."
                },
                "background": {
                    "type": "boolean",
                    "description": "Start the child and return its id without waiting."
                },
                "child": {
                    "type": "string",
                    "description": "A child id from a background start. Returns its state, last action, and result when finished."
                },
                "kill": {
                    "type": "boolean",
                    "description": "With child: stop that child now."
                }
            },
            "additionalProperties": false
        }),
    )
}

impl<P: Provider> Loop<P> {
    pub(super) fn task_available(&self) -> bool {
        self.tasks && self.depth == 0 && self.seat(self.round_role()).0.supports_children()
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
        if calls.len() > MAX_PARALLEL {
            return rejected(
                calls.len(),
                &format!("a task batch can contain at most {MAX_PARALLEL} assignments"),
            );
        }
        let mut args = Vec::with_capacity(calls.len());
        for call in calls {
            match task_args(call) {
                Ok(value) => args.push(value),
                Err(error) => return rejected(calls.len(), &error.to_string()),
            }
        }
        if args
            .iter()
            .all(|args| args.child.is_none() && !args.background)
        {
            return self.task_batch(args, events, emit).await;
        }
        let mut results = Vec::with_capacity(args.len());
        for args in args {
            let result = match args.child.clone() {
                Some(id) => self.task_poll(&id, args.kill, events, emit).await,
                None if args.background => self.task_spawn(args),
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
            results.push(result);
        }
        results
    }

    async fn task_batch(
        &mut self,
        args: Vec<Args>,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
    ) -> Vec<Result<Execution>> {
        let count = args.len();
        let implement = args
            .iter()
            .filter(|args| args.access == Access::Implement)
            .count();
        let budget = match self.task_budget(count) {
            Ok(budget) => budget,
            Err(error) => return rejected(count, &error.to_string()),
        };
        let trees = if implement == 0 {
            Vec::new()
        } else if worktree::in_git(&self.workspace) {
            match worktree::add_children(&self.workspace, implement) {
                Ok(trees) => trees,
                Err(error) => return rejected(count, &error.to_string()),
            }
        } else if implement == 1 {
            Vec::new()
        } else {
            return rejected(
                count,
                "concurrent implement children require a Git repository",
            );
        };
        let mut trees = trees.into_iter();
        let mut runs = Vec::with_capacity(count);
        for args in args {
            let access = args.access;
            let tree = (access == Access::Implement)
                .then(|| trees.next())
                .flatten();
            runs.push((access, self.task_start(args, &budget, tree)));
        }
        let sender = self.mailbox.0.clone();
        let runs = join_all(runs.into_iter().map(|(access, run)| {
            let sender = sender.clone();
            async move {
                let done = match run {
                    Ok(run) => Ok(run.execute(sender).await),
                    Err(error) => Err(error),
                };
                (access, done)
            }
        }));
        tokio::pin!(runs);
        let mut done = loop {
            tokio::select! {
                done = &mut runs => break done,
                Some(event) = self.mailbox.1.recv() => {
                    self.saw(&event);
                    emit(&event);
                    events.push(event);
                }
            }
        };
        self.drain(events, emit);
        self.task_reconcile(&mut done);
        let mut results = Vec::with_capacity(done.len());
        for (_, done) in done {
            results.push(done.and_then(|done| self.task_settle(done)));
        }
        results
    }

    fn task_spawn(&mut self, args: Args) -> Result<Execution> {
        if self.spawned.len() >= MAX_PARALLEL {
            return Err(Error::Denied(format!(
                "at most {MAX_PARALLEL} children run at once; poll or kill one first"
            )));
        }
        let budget = self.task_budget(1)?;
        let tree = if args.access != Access::Implement {
            None
        } else if worktree::in_git(&self.workspace) {
            worktree::add_children(&self.workspace, 1)?.pop()
        } else {
            return Err(Error::Denied(
                "background implement children require a Git repository".to_string(),
            ));
        };
        let run = self.task_start(args, &budget, tree)?;
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
                },
                usage: Usage::default(),
                millis: 0,
            });
        }
        if kill {
            let Some(spawned) = self.spawned.remove(id) else {
                return Err(Error::Tool(format!("unknown child `{id}`")));
            };
            let done = killed(spawned, &self.workspace, "killed by the parent").await;
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
                    millis: super::elapsed_millis(spawned.started),
                }
            }
        }
    }

    pub(super) async fn await_children(
        &mut self,
        events: &mut Vec<Event>,
        emit: &mut impl FnMut(&Event),
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
                if execution.output.mutated {
                    self.session.mark_edit().map_err(super::session_err)?;
                }
                self.push_event(
                    events,
                    emit,
                    Event::Note {
                        text: format!(
                            "child {} finished: {}",
                            short(&id),
                            bounded(&execution.output.text, RESULT_MAX_CHARS)
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

    pub(super) async fn kill_spawned(&mut self, reason: &str) -> Result<Vec<ChildRecord>> {
        let mut records = Vec::new();
        let ids: Vec<String> = self.spawned.keys().cloned().collect();
        for id in ids {
            let Some(spawned) = self.spawned.remove(&id) else {
                continue;
            };
            let done = killed(spawned, &self.workspace, reason).await;
            records.push(done.record.clone());
            drop(self.task_settle(done)?);
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
                "Edit only this worktree. You may build, test, and lint here; background commands are unavailable. The parent reconciles every writable child, then verifies once."
            }
            Access::Research => "This assignment is read-only. Do not change the workspace.",
        };
        let mut child = Self::with_parts(
            provider,
            model.clone(),
            &workspace,
            format!(
                "{}\n\nHarness: You are a depth-1 child. Finish only the assigned task. {instruction} The parent owns final verification and commit.",
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
        child.plan = self.plan;
        child.max_rounds = MAX_ROUNDS;
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
            deadline: Duration::from_secs(budget.max_secs.unwrap_or(MAX_SECS)),
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

    fn task_settle(&mut self, done: Done) -> Result<Execution> {
        let record = done.record;
        self.session
            .finish_child(record.clone())
            .map_err(super::session_err)?;
        self.session
            .charge(
                Meter {
                    input: done.billed,
                    output: 0,
                    cached: 0,
                    millis: 0,
                },
                &format!("child {}", record.id),
                &record.model,
            )
            .map_err(super::session_err)?;
        self.spent_tokens = self.session.replay().spent_tokens;

        let text = serde_json::to_string(&record)
            .map_err(|error| Error::Config(format!("cannot encode child receipt: {error}")))?;
        let success = matches!(record.state, ChildState::Completed(_));
        let mutated = record_paths(&record) > 0;
        Ok(Execution {
            output: ToolOutput {
                text,
                success,
                mutated,
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
        let tokens = self
            .budget
            .max_tokens
            .map(|limit| limit.saturating_sub(self.spent_tokens) / children);
        let secs = self
            .budget
            .max_secs
            .map(|limit| limit.saturating_sub(self.elapsed_secs()))
            .unwrap_or(MAX_SECS)
            .min(MAX_SECS);
        if tokens == Some(0) || secs == 0 {
            return Err(Error::Denied(
                "the parent budget has no capacity for a child".to_string(),
            ));
        }
        Ok(Budget {
            max_tokens: tokens,
            max_secs: Some(secs),
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
            text: bounded(text, RESULT_MAX_CHARS),
            changed_paths: paths
                .into_iter()
                .take(PATHS_MAX)
                .map(|path| bounded(&path, PATH_MAX_CHARS))
                .collect(),
            changed_paths_total: total,
            rounds,
            usage,
            billed_tokens: replay.spent_tokens,
            elapsed_millis: replay.elapsed_millis,
            diagnostic: diagnostic.map(|text| bounded(text, RESULT_MAX_CHARS)),
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
        let outcome = tokio::time::timeout(
            self.deadline,
            Box::pin(self.child.turn_with(self.assignment.as_str(), emit)),
        )
        .await;
        let access = self.record.access;
        let (state, usage) = match outcome {
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
                let reason = format!("child deadline reached after {}s", self.deadline.as_secs());
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
            millis: super::elapsed_millis(self.started),
        }
    }
}

async fn killed(spawned: Spawned, parent: &Path, reason: &str) -> Done {
    spawned.handle.abort();
    let mut record = spawned.record;
    let mut diagnostic = reason.to_string();
    if let Ok(done) = spawned.handle.await {
        return done;
    }
    if record.access == Access::Implement && record.workspace != parent && record.workspace.exists()
    {
        diagnostic.push_str(&format!(
            "; the worktree at {} holds changes and was kept",
            record.workspace.display()
        ));
    }
    let billed = spent_at(&spawned.path);
    let mut result = empty(Some(&diagnostic));
    result.billed_tokens = billed;
    record.state = ChildState::Aborted(result);
    Done {
        record,
        tree: None,
        paths: Vec::new(),
        usage: Usage::default(),
        billed,
        millis: super::elapsed_millis(spawned.started),
    }
}

fn spent_at(path: &Path) -> u64 {
    let mut state = State::default();
    for line in log::read(path).unwrap_or_default() {
        state.apply(line);
    }
    state.spent
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
        diagnostic: diagnostic.map(|text| bounded(text, RESULT_MAX_CHARS)),
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
        result.diagnostic = Some(bounded(diagnostic, RESULT_MAX_CHARS));
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
    result.diagnostic = Some(bounded(diagnostic, RESULT_MAX_CHARS));
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
    if assignment.chars().count() > ASSIGNMENT_MAX_CHARS {
        return Err(Error::Tool(format!(
            "task assignment exceeds {ASSIGNMENT_MAX_CHARS} characters"
        )));
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

#[cfg(test)]
#[path = "child_tests.rs"]
mod tests;
