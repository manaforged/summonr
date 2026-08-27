//! The session driver: the one seam between `Loop` and the store.
//!
//! Two files under `<home>/sessions/<id>/`: `session.jsonl`, the
//! conversation, and `work.json`, the job. The driver holds the live state
//! those files describe; resume folds the log, loads the checkpoint, and
//! aborts whatever turn the dead process left open.

use std::path::{Path, PathBuf};

use thiserror::Error;

use super::id::{Attempt, Call, SessionId};
use super::log::{self, Line, Log, State, Usage};
use super::tree::{self, Snapshot};
use super::work::{Check, Decision, Policy, Stage, Work, WorkError};
use crate::WorkProgress;
use crate::provider::{Image, Message, ToolCall, ToolFunction, ToolKind};
use crate::session::Replay;
use crate::workflow::Role;

#[derive(Debug, Error)]
pub(crate) enum DriverError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Work(#[from] WorkError),
    #[error("input is empty")]
    Empty,
    #[error("a turn is already open")]
    Busy,
    #[error("no open turn")]
    NoTurn,
    #[error("no open provider round")]
    NoProvider,
    #[error("no planned tool for this call")]
    NoTool,
    #[error("session has no Work")]
    NoWork,
    #[error("{0}")]
    State(String),
}

/// The request size a provider round may be charged when it fails.
pub(crate) struct Exposure {
    pub(crate) max_input_tokens: u64,
    pub(crate) max_output_tokens: u64,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Agent,
    Summary,
    Judge,
    Review,
    Scout,
}

struct Round {
    attempt: Attempt,
    purpose: Purpose,
    exposure: Exposure,
    model: String,
}

struct Turn {
    /// Work stage the turn opened in.
    stage: Option<Stage>,
    round: Option<Round>,
    /// Planned tool calls and their provider ids.
    calls: Vec<(Call, String)>,
}

/// A Verify verdict prepared against the live workspace.
pub(crate) struct Verdict {
    pub(crate) passed: bool,
    /// Set when the workspace moved after the candidate was captured.
    pub(crate) diagnostic: Option<String>,
}

pub(crate) struct SessionDriver {
    log: Log,
    state: State,
    work: Option<Work>,
    workspace: PathBuf,
    turn: Option<Turn>,
    next: u64,
}

impl SessionDriver {
    /// Start a new session under `home`.
    pub(crate) fn open(
        home: &Path,
        workspace: &Path,
        model: &str,
        system: &str,
    ) -> Result<Self, DriverError> {
        let id = SessionId::new();
        let path = log::session_path(home, id);
        let mut driver = Self {
            log: Log::create(&path)?,
            state: State::default(),
            work: None,
            workspace: canonical(workspace),
            turn: None,
            next: 1,
        };
        driver.append(Line::Session {
            id,
            model: model.to_string(),
            system: system.to_string(),
            workspace: driver.workspace.clone(),
            parent: None,
        })?;
        Ok(driver)
    }

    /// Reopen a session and settle whatever the last process left open.
    pub(crate) fn resume(
        home: &Path,
        id: SessionId,
        workspace: &Path,
    ) -> Result<Self, DriverError> {
        let path = log::session_path(home, id);
        let mut state = State::default();
        for line in log::read(&path)? {
            state.apply(line);
        }
        if state.id != Some(id) {
            return Err(DriverError::State(format!(
                "{} is not the log of session {id}",
                path.display()
            )));
        }
        // Resume in the session's own workspace, not wherever the command ran.
        // The recorded path is already canonical; the Work checkpoint and the
        // change snapshot both describe that tree.
        let workspace = if state.workspace.as_os_str().is_empty() {
            canonical(workspace)
        } else {
            state.workspace.clone()
        };
        let mut driver = Self {
            log: Log::open(&path)?,
            work: Work::load(&log::work_path(&path))?,
            state,
            workspace,
            turn: None,
            next: 1,
        };
        driver.recover()?;
        Ok(driver)
    }

    fn recover(&mut self) -> Result<(), DriverError> {
        if self.state.opener.is_some() {
            self.turn = Some(Turn {
                stage: self.work.as_ref().and_then(Work::stage),
                round: None,
                calls: Vec::new(),
            });
            return self.abort("process stopped before the turn finished");
        }
        if let Some(work) = &mut self.work
            && work.interrupt()
        {
            self.checkpoint()?;
        }
        Ok(())
    }

    /// Open a turn on the active Work. Returns harness text queued for it.
    pub(crate) fn start_input(
        &mut self,
        text: &str,
        images: &[Image],
    ) -> Result<Vec<String>, DriverError> {
        if text.trim().is_empty() {
            return Err(DriverError::Empty);
        }
        if self.turn.is_some() {
            return Err(DriverError::Busy);
        }
        let work = self.work.as_mut().ok_or(DriverError::NoWork)?;
        let stage = work.stage().ok_or(DriverError::NoWork)?;
        let queued = if stage == Stage::Execute {
            std::mem::take(&mut work.pending)
        } else {
            Vec::new()
        };
        // Persist that the queue is consumed before writing the lines. A crash
        // here loses the queued continuation (the goal loop re-derives it),
        // which beats re-appending it and duplicating it in the conversation.
        if !queued.is_empty() {
            self.checkpoint()?;
        }
        for text in &queued {
            self.append(Line::User {
                text: text.clone(),
                images: Vec::new(),
                start: false,
            })?;
        }
        self.append(Line::User {
            text: text.to_string(),
            images: images.to_vec(),
            start: true,
        })?;
        self.turn = Some(Turn {
            stage: Some(stage),
            round: None,
            calls: Vec::new(),
        });
        Ok(queued)
    }

    /// Add user or peer text to the open turn.
    pub(crate) fn steer(&mut self, text: &str) -> Result<(), DriverError> {
        if text.trim().is_empty() {
            return Err(DriverError::Empty);
        }
        if self.turn.is_none() {
            return Err(DriverError::NoTurn);
        }
        self.append(Line::User {
            text: text.to_string(),
            images: Vec::new(),
            start: false,
        })
    }

    /// Open an agent round on `model`.
    pub(crate) fn start_provider(
        &mut self,
        model: &str,
        exposure: Exposure,
    ) -> Result<Attempt, DriverError> {
        self.start_round(Purpose::Agent, model, exposure)
    }

    pub(crate) fn start_summary(
        &mut self,
        model: &str,
        exposure: Exposure,
    ) -> Result<Attempt, DriverError> {
        self.start_round(Purpose::Summary, model, exposure)
    }

    pub(crate) fn start_judge(
        &mut self,
        model: &str,
        exposure: Exposure,
    ) -> Result<Attempt, DriverError> {
        self.start_round(Purpose::Judge, model, exposure)
    }

    /// Open a private round for a scout or review seat.
    pub(crate) fn start_aside(
        &mut self,
        role: Role,
        model: &str,
        exposure: Exposure,
    ) -> Result<Attempt, DriverError> {
        let purpose = match role {
            Role::Scout => Purpose::Scout,
            Role::Review => Purpose::Review,
            Role::Plan | Role::Implement | Role::Judge | Role::Summary => {
                return Err(DriverError::State(format!("{role} is not an aside")));
            }
        };
        self.start_round(purpose, model, exposure)
    }

    fn start_round(
        &mut self,
        purpose: Purpose,
        model: &str,
        exposure: Exposure,
    ) -> Result<Attempt, DriverError> {
        let attempt = Attempt(self.take());
        let turn = self.turn.as_mut().ok_or(DriverError::NoTurn)?;
        if turn.round.is_some() {
            return Err(DriverError::State(
                "a provider round is already open".to_string(),
            ));
        }
        turn.round = Some(Round {
            attempt,
            purpose,
            exposure,
            model: model.to_string(),
        });
        Ok(attempt)
    }

    fn end_round(&mut self, attempt: Attempt) -> Result<Round, DriverError> {
        let turn = self.turn.as_mut().ok_or(DriverError::NoTurn)?;
        match turn.round.take() {
            Some(round) if round.attempt == attempt => Ok(round),
            other => {
                turn.round = other;
                Err(DriverError::NoProvider)
            }
        }
    }

    /// Close a provider round that answered without tool calls.
    pub(crate) fn finish_provider(
        &mut self,
        attempt: Attempt,
        text: &str,
        reasoning: &str,
        usage: Usage,
    ) -> Result<(), DriverError> {
        let round = self.end_round(attempt)?;
        let line = match round.purpose {
            Purpose::Agent => Line::Assistant {
                text: text.to_string(),
                reasoning: reasoning.to_string(),
                tool_calls: Vec::new(),
                usage,
                model: round.model,
            },
            Purpose::Summary => Line::Charge {
                usage,
                note: "compaction summary".to_string(),
                model: round.model,
            },
            Purpose::Judge => Line::Charge {
                usage,
                note: "goal judge".to_string(),
                model: round.model,
            },
            Purpose::Review => Line::Charge {
                usage,
                note: "review".to_string(),
                model: round.model,
            },
            Purpose::Scout => Line::Charge {
                usage,
                note: "scout".to_string(),
                model: round.model,
            },
        };
        self.append(line)
    }

    /// Close a provider round and plan its tool calls in provider order.
    pub(crate) fn finish_provider_with_tools(
        &mut self,
        attempt: Attempt,
        text: &str,
        reasoning: &str,
        usage: Usage,
        tools: &[(&str, &str, &str)],
    ) -> Result<Vec<Call>, DriverError> {
        let round = self.end_round(attempt)?;
        if round.purpose != Purpose::Agent {
            return Err(DriverError::NoProvider);
        }
        let planned: Vec<(Call, String)> = tools
            .iter()
            .map(|(_, _, id)| (Call(self.take()), (*id).to_string()))
            .collect();
        let tool_calls = tools
            .iter()
            .map(|(name, arguments, id)| ToolCall {
                id: (*id).to_string(),
                kind: ToolKind::Function,
                function: ToolFunction {
                    name: (*name).to_string(),
                    arguments: (*arguments).to_string(),
                },
            })
            .collect();
        self.append(Line::Assistant {
            text: text.to_string(),
            reasoning: reasoning.to_string(),
            tool_calls,
            usage,
            model: round.model,
        })?;
        let ids = planned.iter().map(|(call, _)| *call).collect();
        if let Some(turn) = self.turn.as_mut() {
            turn.calls = planned;
        }
        Ok(ids)
    }

    /// Close a provider round that failed; it is charged its full exposure.
    pub(crate) fn fail_provider(
        &mut self,
        attempt: Attempt,
        diagnostic: &str,
        millis: u64,
    ) -> Result<(), DriverError> {
        let round = self.end_round(attempt)?;
        self.append(Line::Charge {
            usage: Usage {
                input: round.exposure.max_input_tokens,
                output: round.exposure.max_output_tokens,
                cached: 0,
                millis,
            },
            note: diagnostic.to_string(),
            model: round.model,
        })
    }

    /// Record every tool result of the batch in provider-call order.
    pub(crate) fn finish_tools(
        &mut self,
        results: &[(Call, &str, bool, u64)],
    ) -> Result<(), DriverError> {
        for (call, text, success, millis) in results {
            let id = self
                .turn
                .as_ref()
                .and_then(|turn| turn.calls.iter().find(|(planned, _)| planned == call))
                .map(|(_, id)| id.clone())
                .ok_or(DriverError::NoTool)?;
            self.append(Line::Tool {
                call: id,
                text: (*text).to_string(),
                success: *success,
                millis: *millis,
            })?;
        }
        Ok(())
    }

    /// Capture the Execute result and move Work to Verify. Returns the
    /// changed paths.
    pub(crate) fn enter_verify(&mut self) -> Result<Vec<String>, DriverError> {
        if self.turn.is_none() {
            return Err(DriverError::NoTurn);
        }
        let capture = self.snapshot()?;
        let paths = self
            .work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .verify(capture)?;
        self.checkpoint()?;
        Ok(paths)
    }

    /// Check the candidate is still what the gates ran against.
    pub(crate) fn prepare_verdict(&mut self, passed: bool) -> Result<Verdict, DriverError> {
        let now = self.snapshot()?;
        // Stable when the files the turn changed are still as captured. A gate
        // that rewrites an untouched file (Cargo.lock, a generated fixture)
        // must not invalidate a verdict whose checks passed.
        let stable = self
            .work
            .as_ref()
            .and_then(|work| work.candidate.as_ref())
            .is_some_and(|candidate| candidate.snapshot.matches(&now, &candidate.paths));
        if stable {
            return Ok(Verdict {
                passed,
                diagnostic: None,
            });
        }
        Ok(Verdict {
            passed: false,
            diagnostic: Some("workspace changed after candidate capture".to_string()),
        })
    }

    /// Move Work from Verify to Review once the gates passed.
    pub(crate) fn enter_review(&mut self) -> Result<(), DriverError> {
        if self.turn.is_none() {
            return Err(DriverError::NoTurn);
        }
        self.work.as_mut().ok_or(DriverError::NoWork)?.review()?;
        self.checkpoint()
    }

    /// The reviewer sent the change back: queue its findings, record the
    /// receipt, and close the turn.
    pub(crate) fn commit_review(
        &mut self,
        findings: &str,
        receipt: Option<&str>,
    ) -> Result<(), DriverError> {
        if self.turn.is_none() {
            return Err(DriverError::NoTurn);
        }
        self.work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .reject(findings)?;
        self.checkpoint()?;
        if let Some(text) = receipt {
            self.append(Line::Receipt {
                text: text.to_string(),
            })?;
        }
        self.close()
    }

    /// Commit the verdict, the visible receipt, and close the turn.
    pub(crate) fn commit_verdict(
        &mut self,
        verdict: Verdict,
        decision: Decision,
        receipt: Option<&str>,
    ) -> Result<(), DriverError> {
        if self.turn.is_none() {
            return Err(DriverError::NoTurn);
        }
        self.work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .verdict(verdict.passed, decision)?;
        self.checkpoint()?;
        if let Some(text) = receipt {
            self.append(Line::Receipt {
                text: text.to_string(),
            })?;
        }
        self.close()
    }

    /// Close the open turn.
    pub(crate) fn close(&mut self) -> Result<(), DriverError> {
        if self.turn.is_none() {
            return Err(DriverError::NoTurn);
        }
        self.append(Line::Close { reason: None })?;
        self.turn = None;
        Ok(())
    }

    /// Abort the open turn: unanswered tool calls get an `aborted` result so
    /// the conversation stays well-formed, and an interrupted Verify returns
    /// to Execute. Idle sessions are left alone.
    pub(crate) fn abort(&mut self, reason: &str) -> Result<(), DriverError> {
        if self.turn.is_none() {
            return Ok(());
        }
        for call in self.state.unanswered.clone() {
            self.append(Line::Tool {
                call,
                text: "aborted".to_string(),
                success: false,
                millis: 0,
            })?;
        }
        self.append(Line::Close {
            reason: Some(reason.to_string()),
        })?;
        self.turn = None;
        if let Some(work) = &mut self.work
            && work.interrupt()
        {
            self.checkpoint()?;
        }
        Ok(())
    }

    /// Replace the older conversation with a summary.
    pub(crate) fn compact(&mut self, summary: &str, tail: &[Message]) -> Result<(), DriverError> {
        self.append(Line::Summary {
            text: summary.to_string(),
            tail: tail.to_vec(),
        })
    }

    /// Drop the idle conversation.
    pub(crate) fn clear(&mut self) -> Result<(), DriverError> {
        if self.turn.is_some() {
            return Err(DriverError::Busy);
        }
        self.append(Line::Clear)
    }

    /// Copy this conversation into a new child session and return its log
    /// path. Work stays here.
    pub(crate) fn fork(&self) -> Result<PathBuf, DriverError> {
        let home = log::home_of(self.log.path())
            .ok_or_else(|| DriverError::State("session log has no home".to_string()))?;
        let child = SessionId::new();
        let path = log::session_path(home, child);
        let mut log = Log::create(&path)?;
        log.append(&Line::Session {
            id: child,
            model: self.state.model.clone(),
            system: self.state.system.clone(),
            workspace: self.state.workspace.clone(),
            parent: self.state.id,
        })?;
        for line in log::read(self.log.path())? {
            if !matches!(line, Line::Session { .. }) {
                log.append(&line)?;
            }
        }
        Ok(path)
    }

    pub(crate) fn set_budget(
        &mut self,
        tokens: Option<u64>,
        millis: Option<u64>,
    ) -> Result<(), DriverError> {
        if (self.state.tokens, self.state.millis) == (tokens, millis) {
            return Ok(());
        }
        self.append(Line::Budget { tokens, millis })
    }

    pub(crate) fn budget_limits(&self) -> (Option<u64>, Option<u64>) {
        (self.state.tokens, self.state.millis)
    }

    /// Record the active workflow name; `None` clears it.
    pub(crate) fn set_workflow(&mut self, name: Option<&str>) -> Result<(), DriverError> {
        if self.state.workflow.as_deref() == name {
            return Ok(());
        }
        self.append(Line::Workflow {
            name: name.map(str::to_string),
        })
    }

    pub(crate) fn workflow(&self) -> Option<&str> {
        self.state.workflow.as_deref()
    }

    /// Work stage the open turn began in.
    pub(crate) fn stage(&self) -> Option<Stage> {
        self.turn.as_ref().and_then(|turn| turn.stage)
    }

    #[cfg(test)]
    pub(crate) fn busy(&self) -> bool {
        self.turn.is_some()
    }

    pub(crate) fn replay(&self) -> Replay {
        self.state.replay()
    }

    pub(crate) fn path(&self) -> &Path {
        self.log.path()
    }

    #[cfg(test)]
    pub(crate) fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub(crate) fn id(&self) -> SessionId {
        self.state.id.expect("an open session has a header")
    }

    pub(crate) fn work_progress(&self) -> Result<WorkProgress, DriverError> {
        Ok(self.work.as_ref().ok_or(DriverError::NoWork)?.progress())
    }

    pub(crate) fn abandon_work(&mut self, reason: &str) -> Result<(), DriverError> {
        self.work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .abandon(reason);
        self.checkpoint()
    }

    /// The active Work's objective.
    pub(crate) fn objective(&self) -> Result<String, DriverError> {
        Ok(self
            .work
            .as_ref()
            .ok_or(DriverError::NoWork)?
            .objective
            .clone())
    }

    /// Whether the active Work is a job that has not been scouted: numbered
    /// steps, a goal, or a single turn in plan mode.
    pub(crate) fn needs_scout(&self, plan: bool) -> bool {
        self.active().is_some_and(|work| {
            work.brief.is_none()
                && match work.policy {
                    Policy::Steps | Policy::Goal { .. } => true,
                    Policy::Single => plan,
                }
        })
    }

    /// Keep the scout's brief with the Work.
    pub(crate) fn set_brief(&mut self, brief: &str) -> Result<(), DriverError> {
        self.work.as_mut().ok_or(DriverError::NoWork)?.brief = Some(brief.to_string());
        self.checkpoint()
    }

    pub(crate) fn goal_objective(&self) -> Result<Option<String>, DriverError> {
        let work = self.work.as_ref().ok_or(DriverError::NoWork)?;
        Ok(matches!(work.policy, Policy::Goal { .. }).then(|| work.objective.clone()))
    }

    pub(crate) fn completion_checks(&self) -> Result<Vec<Check>, DriverError> {
        Ok(self
            .work
            .as_ref()
            .ok_or(DriverError::NoWork)?
            .completion_checks())
    }

    /// Open or reuse the single-turn Work that owns an interactive Plan.
    pub(crate) fn prepare_plan_work(&mut self, objective: &str) -> Result<(), DriverError> {
        match self.active() {
            None => self.set_work(objective, Policy::Single),
            Some(work) if work.policy == Policy::Single && work.stage() == Some(Stage::Plan) => {
                Ok(())
            }
            Some(_) => Err(DriverError::State(
                "another Work is already active".to_string(),
            )),
        }
    }

    /// Finish the Plan round and select its text as the plan.
    pub(crate) fn finish_plan(
        &mut self,
        attempt: Attempt,
        markdown: &str,
        reasoning: &str,
        usage: Usage,
        commands: &[String],
    ) -> Result<(), DriverError> {
        self.finish_provider(attempt, markdown, reasoning, usage)?;
        self.work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .select(markdown, commands)?;
        self.checkpoint()?;
        self.close()
    }

    /// Enter the selected interactive Plan into Execute.
    pub(crate) fn accept_plan(&mut self) -> Result<(), DriverError> {
        let selected = self.active().is_some_and(|work| {
            work.policy == Policy::Single
                && work.stage() == Some(Stage::Plan)
                && work.plan.is_some()
        });
        if !selected {
            return Err(DriverError::State(
                "there is no selected interactive Plan to accept".to_string(),
            ));
        }
        self.execute()
    }

    /// Open or resume the Work a Steps or Goal runner owns. `text` is the
    /// plan for Steps and the objective for Goal.
    pub(crate) fn prepare_external_work(
        &mut self,
        text: &str,
        policy: Policy,
        commands: &[String],
    ) -> Result<(), DriverError> {
        if policy == Policy::Single {
            return Err(DriverError::State(
                "external Work requires Steps or Goal policy".to_string(),
            ));
        }
        match self.active() {
            Some(work) if work.policy != policy => {
                return Err(DriverError::State(
                    "another Work policy is already active".to_string(),
                ));
            }
            Some(work) if work.objective != text => {
                return Err(DriverError::State(
                    "another Work objective is already active".to_string(),
                ));
            }
            Some(_) => {}
            None => self.set_work(text, policy)?,
        }
        let work = self.work.as_mut().ok_or(DriverError::NoWork)?;
        if work.plan.is_none() {
            work.select(text, commands)?;
        }
        let expected = super::work::checks(text, policy, commands)?;
        if work.plan.as_deref() != Some(text) || work.checks != expected {
            return Err(DriverError::State(
                "the plan or completion checks changed while Work was active".to_string(),
            ));
        }
        match work.stage() {
            Some(Stage::Plan) => {
                self.checkpoint()?;
                self.execute()
            }
            Some(Stage::Execute) => self.checkpoint(),
            Some(Stage::Verify | Stage::Review) => Err(DriverError::State(
                "Work is still in Verify after an interrupted turn".to_string(),
            )),
            None => Err(DriverError::NoWork),
        }
    }

    /// Attach one turn to the active Work, or open a single-turn Work for
    /// this objective.
    pub(crate) fn prepare_turn_work(
        &mut self,
        objective: &str,
        commands: &[String],
    ) -> Result<(), DriverError> {
        let created = self.active().is_none();
        if created {
            self.set_work(objective, Policy::Single)?;
            self.work
                .as_mut()
                .ok_or(DriverError::NoWork)?
                .select(objective, commands)?;
        }
        let work = self.work.as_ref().ok_or(DriverError::NoWork)?;
        if work.policy == Policy::Single
            && work
                .checks
                .iter()
                .map(|check| check.command.as_str())
                .ne(commands.iter().map(String::as_str))
        {
            return Err(DriverError::State(
                "completion checks changed while Work was active".to_string(),
            ));
        }
        match work.stage() {
            Some(Stage::Plan) if work.policy == Policy::Single && !created => Err(
                DriverError::State("accept the selected Plan before implementation".to_string()),
            ),
            Some(Stage::Plan) => self.execute(),
            Some(Stage::Execute) => Ok(()),
            Some(Stage::Verify | Stage::Review) => Err(DriverError::State(
                "Work is still in Verify after an interrupted turn".to_string(),
            )),
            None => Err(DriverError::NoWork),
        }
    }

    fn active(&self) -> Option<&Work> {
        self.work.as_ref().filter(|work| !work.terminal())
    }

    fn set_work(&mut self, objective: &str, policy: Policy) -> Result<(), DriverError> {
        self.work = Some(Work::new(objective, policy));
        self.checkpoint()
    }

    fn execute(&mut self) -> Result<(), DriverError> {
        let base = self.snapshot()?;
        self.work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .execute(base)?;
        self.checkpoint()
    }

    fn snapshot(&self) -> Result<Snapshot, DriverError> {
        let sessions = self
            .log
            .path()
            .parent()
            .and_then(Path::parent)
            .map(canonical);
        Ok(tree::snapshot(&self.workspace, sessions.as_deref())?)
    }

    fn checkpoint(&self) -> Result<(), DriverError> {
        if let Some(work) = &self.work {
            work.save(&log::work_path(self.log.path()))?;
        }
        Ok(())
    }

    fn append(&mut self, line: Line) -> Result<(), DriverError> {
        self.log.append(&line)?;
        self.state.apply(line);
        Ok(())
    }

    fn take(&mut self) -> u64 {
        let value = self.next;
        self.next = self.next.saturating_add(1);
        value
    }
}

/// Whether the session at `path` holds this workspace's active Work for
/// `policy` and `text`.
pub(crate) fn work_at(path: &Path, workspace: &Path, policy: Policy, text: &str) -> bool {
    let Ok(Line::Session {
        workspace: recorded,
        ..
    }) = log::header(path)
    else {
        return false;
    };
    if recorded != canonical(workspace) {
        return false;
    }
    Work::load(&log::work_path(path))
        .ok()
        .flatten()
        .is_some_and(|work| work.policy == policy && work.objective == text && !work.terminal())
}

pub(crate) fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
