use std::time::{Duration, Instant};

use futures::future::join_all;
use serde::Deserialize;

use super::*;
use crate::child::{
    ASSIGNMENT_MAX_CHARS, Access, ChildRecord, ChildResult, ChildState, MAX_PARALLEL, MAX_ROUNDS,
    MAX_SECS, MAX_TOKENS, PATH_MAX_CHARS, PATHS_MAX, RESULT_MAX_CHARS, bounded,
};
use crate::durable::log::{self, Usage as Meter};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    assignment: String,
    #[serde(default)]
    access: Access,
}

pub(super) struct Execution {
    pub(super) output: crate::tools::ToolOutput,
    pub(super) usage: Usage,
    pub(super) millis: u64,
}

struct Run<P: Provider> {
    child: Loop<P>,
    record: ChildRecord,
    tree: Option<crate::worktree::Tree>,
    assignment: String,
    deadline: Duration,
    started: Instant,
}

struct Done {
    record: ChildRecord,
    tree: Option<crate::worktree::Tree>,
    paths: Vec<String>,
    usage: Usage,
    billed: u64,
    millis: u64,
}

pub(super) fn definition() -> ToolDefinition {
    crate::tools::tool(
        "task",
        "Delegate one bounded assignment to a fresh child session. Emit up to four task calls together. Implement children use isolated Git worktrees and file tools only; research children are read-only. The parent reconciles changes, verifies once, and commits.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "assignment": {
                    "type": "string",
                    "minLength": 1,
                    "maxLength": ASSIGNMENT_MAX_CHARS
                },
                "access": {
                    "type": "string",
                    "enum": ["implement", "research"],
                    "default": "implement",
                    "description": "Implement edits an isolated worktree without running builds. Research is read-only."
                }
            },
            "required": ["assignment"],
            "additionalProperties": false
        }),
    )
}

impl<P: Provider> Loop<P> {
    pub(super) fn task_available(&self) -> bool {
        self.depth == 0 && self.seat(self.round_role()).0.supports_children()
    }

    pub(super) async fn task_execute(&mut self, call: &ToolCall) -> Result<Execution> {
        self.tasks_execute(std::slice::from_ref(call))
            .await
            .pop()
            .ok_or_else(|| Error::Config("task execution returned no result".to_string()))?
    }

    pub(super) async fn tasks_execute(&mut self, calls: &[ToolCall]) -> Vec<Result<Execution>> {
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
        let implement = args
            .iter()
            .filter(|args| args.access == Access::Implement)
            .count();
        let budget = match self.task_budget(calls.len()) {
            Ok(budget) => budget,
            Err(error) => return rejected(calls.len(), &error.to_string()),
        };
        let trees = if implement == 0 {
            Vec::new()
        } else if crate::worktree::in_git(&self.workspace) {
            match crate::worktree::add_children(&self.workspace, implement) {
                Ok(trees) => trees,
                Err(error) => return rejected(calls.len(), &error.to_string()),
            }
        } else if implement == 1 {
            Vec::new()
        } else {
            return rejected(
                calls.len(),
                "concurrent implement children require a Git repository",
            );
        };
        let mut trees = trees.into_iter();
        let mut runs = Vec::with_capacity(args.len());
        for args in args {
            let access = args.access;
            let tree = (access == Access::Implement)
                .then(|| trees.next())
                .flatten();
            runs.push((access, self.task_start(args, &budget, tree)));
        }
        let mut done = join_all(runs.into_iter().map(|(access, run)| async move {
            let done = match run {
                Ok(run) => Ok(run.execute().await),
                Err(error) => Err(error),
            };
            (access, done)
        }))
        .await;
        self.task_reconcile(&mut done);
        let mut results = Vec::with_capacity(done.len());
        for (_, done) in done {
            results.push(done.and_then(|done| self.task_settle(done)));
        }
        results
    }

    fn task_start(
        &mut self,
        args: Args,
        budget: &Budget,
        tree: Option<crate::worktree::Tree>,
    ) -> Result<Run<P>> {
        let cleanup = tree.clone();
        let result = self.task_start_in(args, budget, tree);
        if result.is_err()
            && let Some(tree) = cleanup
        {
            drop(crate::worktree::remove_child(&tree));
        }
        result
    }

    fn task_start_in(
        &mut self,
        args: Args,
        budget: &Budget,
        tree: Option<crate::worktree::Tree>,
    ) -> Result<Run<P>> {
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
                "Edit only this worktree. Build, test, lint, formatter, command, and language-server tools are unavailable. The parent reconciles every writable child, then verifies once."
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
            tree,
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
        match crate::worktree::reconcile_children(&self.workspace, &children) {
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
            output: crate::tools::ToolOutput {
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
        let child_max = self.context_window.map(u64::from).unwrap_or(MAX_TOKENS);
        let tokens = self
            .budget
            .max_tokens
            .map(|limit| limit.saturating_sub(self.spent_tokens) / children)
            .unwrap_or(child_max)
            .min(child_max);
        let secs = self
            .budget
            .max_secs
            .map(|limit| limit.saturating_sub(self.elapsed_secs()))
            .unwrap_or(MAX_SECS)
            .min(MAX_SECS);
        if tokens == 0 || secs == 0 {
            return Err(Error::Denied(
                "the parent budget has no capacity for a child".to_string(),
            ));
        }
        Ok(Budget {
            max_tokens: Some(tokens),
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
            billed_tokens: self.session.replay().spent_tokens,
            elapsed_millis: self.session.replay().elapsed_millis,
            diagnostic: diagnostic.map(|text| bounded(text, RESULT_MAX_CHARS)),
        }
    }
}

impl<P: Provider> Run<P> {
    async fn execute(mut self) -> Done {
        let outcome =
            tokio::time::timeout(self.deadline, Box::pin(self.child.turn(&self.assignment))).await;
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
            tree: self.tree,
            paths,
            usage,
            billed: self.child.session.replay().spent_tokens,
            millis: super::elapsed_millis(self.started),
        }
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
