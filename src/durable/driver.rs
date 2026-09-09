use std::path::{Path, PathBuf};

use thiserror::Error;

use super::id::{Attempt, Call, SessionId};
use super::log::{self, Line, Log, State, Usage};
use super::tree::{self, Snapshot};
use super::work::{Check, Decision, Policy, Stage, Work, WorkError};
use crate::WorkProgress;
use crate::provider::{Image, Message, ToolCall};
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
    stage: Option<Stage>,
    round: Option<Round>,
    calls: Vec<(Call, String)>,
}

pub(crate) struct Verdict {
    pub(crate) passed: bool,
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
            depth: 0,
        })?;
        Ok(driver)
    }

    pub(crate) fn open_child(
        home: &Path,
        workspace: &Path,
        model: &str,
        system: &str,
        parent: SessionId,
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
            parent: Some(parent),
            depth: 1,
        })?;
        Ok(driver)
    }

    pub(crate) fn resume(
        home: &Path,
        id: SessionId,
        workspace: &Path,
    ) -> Result<Self, DriverError> {
        let caller = workspace;
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
        let workspace = if state.workspace.as_os_str().is_empty() {
            canonical(workspace)
        } else {
            state.workspace.clone()
        };
        let work = Work::load(&log::work_path(&path))?;
        let workspace = if workspace.is_dir() {
            workspace
        } else if work.as_ref().is_none_or(Work::terminal) {
            canonical(caller)
        } else {
            return Err(DriverError::State(format!(
                "session {id} was recorded in {}, which no longer exists",
                workspace.display()
            )));
        };
        let mut driver = Self {
            log: Log::open(&path)?,
            work,
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
            self.abort("process stopped before the turn finished", None)?;
            if let Some(work) = &mut self.work
                && !work.terminal()
            {
                work.pending.push(
                    "[summon: the previous attempt at this turn was interrupted; the workspace is as it left it — continue from there]"
                        .to_string(),
                );
                self.checkpoint()?;
            }
            return Ok(());
        }
        if let Some(work) = &mut self.work
            && work.interrupt()
        {
            self.checkpoint()?;
        }
        self.interrupt_children("process stopped before the child finished")?;
        Ok(())
    }

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

    pub(crate) fn finish_provider_with_tools(
        &mut self,
        attempt: Attempt,
        text: &str,
        reasoning: &str,
        usage: Usage,
        tools: &[ToolCall],
    ) -> Result<Vec<Call>, DriverError> {
        let round = self.end_round(attempt)?;
        if round.purpose != Purpose::Agent {
            return Err(DriverError::NoProvider);
        }
        let planned: Vec<(Call, String)> = tools
            .iter()
            .map(|call| (Call(self.take()), call.id.clone()))
            .collect();
        self.append(Line::Assistant {
            text: text.to_string(),
            reasoning: reasoning.to_string(),
            tool_calls: tools.to_vec(),
            usage,
            model: round.model,
        })?;
        let ids = planned.iter().map(|(call, _)| *call).collect();
        if let Some(turn) = self.turn.as_mut() {
            turn.calls = planned;
        }
        Ok(ids)
    }

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

    pub(crate) fn defer(&mut self, note: &str, jobs: Vec<String>) -> Result<bool, DriverError> {
        if self.turn.is_none() {
            return Err(DriverError::NoTurn);
        }
        let deferred = self
            .work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .defer(note.to_string(), jobs)?;
        if deferred {
            self.checkpoint()?;
            self.close()?;
        }
        Ok(deferred)
    }

    pub(crate) fn jobs(&self) -> Vec<String> {
        self.active()
            .map(|work| work.jobs.clone())
            .unwrap_or_default()
    }

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

    pub(crate) fn prepare_verdict(&self, passed: bool) -> Result<Verdict, DriverError> {
        let now = self.snapshot()?;
        let stable = self
            .work
            .as_ref()
            .and_then(|work| work.candidate.as_ref())
            .is_some_and(|candidate| candidate.snapshot == now);
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

    pub(crate) fn enter_review(&mut self) -> Result<(), DriverError> {
        if self.turn.is_none() {
            return Err(DriverError::NoTurn);
        }
        self.work.as_mut().ok_or(DriverError::NoWork)?.review()?;
        self.checkpoint()
    }

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

    pub(crate) fn close(&mut self) -> Result<(), DriverError> {
        if self.turn.is_none() {
            return Err(DriverError::NoTurn);
        }
        self.append(Line::Close { reason: None })?;
        self.turn = None;
        Ok(())
    }

    pub(crate) fn abort(&mut self, reason: &str, receipt: Option<&str>) -> Result<(), DriverError> {
        if self.turn.is_none() {
            return Ok(());
        }
        if let Some(attempt) = self
            .turn
            .as_ref()
            .and_then(|turn| turn.round.as_ref())
            .map(|round| round.attempt)
        {
            self.fail_provider(attempt, reason, 0)?;
        }
        for call in self.state.unanswered.clone() {
            self.append(Line::Tool {
                call,
                text: "aborted".to_string(),
                success: false,
                millis: 0,
            })?;
        }
        if let Some(text) = receipt {
            self.append(Line::Receipt {
                text: text.to_string(),
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
        self.interrupt_children(reason)?;
        Ok(())
    }

    pub(crate) fn start_child(
        &mut self,
        record: crate::child::ChildRecord,
    ) -> Result<(), DriverError> {
        if !matches!(record.state, crate::child::ChildState::Running) {
            return Err(DriverError::State(
                "a child must start in the running state".to_string(),
            ));
        }
        let workspace = record.workspace == self.workspace
            || record.access == crate::child::Access::Implement
                && crate::worktree::same_repository(&self.workspace, &record.workspace);
        if record.version != 1
            || record.parent != self.id()
            || record.depth != self.depth().saturating_add(1)
            || !workspace
            || record.assignment.trim().is_empty()
            || record.model.trim().is_empty()
        {
            return Err(DriverError::State(
                "child identity does not match the parent session".to_string(),
            ));
        }
        self.append(Line::Child { record })
    }

    pub(crate) fn finish_child(
        &mut self,
        record: crate::child::ChildRecord,
    ) -> Result<(), DriverError> {
        if !record.state.terminal() {
            return Err(DriverError::State(
                "a child result must be terminal".to_string(),
            ));
        }
        match self.state.children.get(&record.id) {
            Some(child) if !child.state.terminal() && child.same_child(&record) => {
                self.append(Line::Child { record })
            }
            Some(child) if !child.same_child(&record) => Err(DriverError::State(format!(
                "child {} changed identity before settlement",
                record.id
            ))),
            Some(_) => Err(DriverError::State(format!(
                "child {} is already terminal",
                record.id
            ))),
            None => Err(DriverError::State(format!(
                "child {} was not started by this session",
                record.id
            ))),
        }
    }

    pub(crate) fn child(&self, id: SessionId) -> Option<crate::child::ChildRecord> {
        self.state.children.get(&id).cloned()
    }

    #[cfg(test)]
    pub(crate) fn children(&self) -> Vec<crate::child::ChildRecord> {
        self.state.children.values().cloned().collect()
    }

    fn interrupt_children(&mut self, reason: &str) -> Result<(), DriverError> {
        let records = self
            .state
            .children
            .values()
            .filter(|child| !child.state.terminal())
            .map(|child| child.interrupt(reason))
            .collect::<Vec<_>>();
        for record in records {
            self.append(Line::Child { record })?;
        }
        Ok(())
    }

    pub(crate) fn charge(
        &mut self,
        usage: Usage,
        note: &str,
        model: &str,
    ) -> Result<(), DriverError> {
        self.append(Line::Charge {
            usage,
            note: note.to_string(),
            model: model.to_string(),
        })
    }

    pub(crate) fn compact(&mut self, summary: &str, tail: &[Message]) -> Result<(), DriverError> {
        self.append(Line::Summary {
            text: summary.to_string(),
            tail: tail.to_vec(),
        })
    }

    pub(crate) fn clear(&mut self) -> Result<(), DriverError> {
        if self.turn.is_some() {
            return Err(DriverError::Busy);
        }
        self.append(Line::Clear)
    }

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
            depth: 0,
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

    pub(crate) fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub(crate) fn id(&self) -> SessionId {
        self.state.id.expect("an open session has a header")
    }

    pub(crate) fn depth(&self) -> u8 {
        self.state.depth
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

    pub(crate) fn objective(&self) -> Result<String, DriverError> {
        Ok(self
            .work
            .as_ref()
            .ok_or(DriverError::NoWork)?
            .objective
            .clone())
    }

    pub(crate) fn needs_scout(&self, plan: bool) -> bool {
        self.active().is_some_and(|work| {
            work.brief.is_none()
                && match work.policy {
                    Policy::Steps | Policy::Goal { .. } => true,
                    Policy::Single => plan,
                }
        })
    }

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

    pub(crate) fn finish_plan(
        &mut self,
        attempt: Attempt,
        markdown: &str,
        reasoning: &str,
        usage: Usage,
        commands: &[String],
        receipt: &str,
    ) -> Result<(), DriverError> {
        self.finish_provider(attempt, markdown, reasoning, usage)?;
        self.work
            .as_mut()
            .ok_or(DriverError::NoWork)?
            .select(markdown, commands)?;
        self.checkpoint()?;
        self.append(Line::Receipt {
            text: receipt.to_string(),
        })?;
        self.close()
    }

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
            Some(work)
                if std::mem::discriminant(&work.policy) != std::mem::discriminant(&policy) =>
            {
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

    pub(crate) fn prepare_turn_work(
        &mut self,
        objective: &str,
        commands: &[String],
    ) -> Result<bool, DriverError> {
        let created = self.active().is_none();
        if created {
            self.set_work(objective, Policy::Single)?;
            self.work
                .as_mut()
                .ok_or(DriverError::NoWork)?
                .select(objective, commands)?;
        }
        let work = self.work.as_mut().ok_or(DriverError::NoWork)?;
        if work.policy == Policy::Single
            && work
                .checks
                .iter()
                .map(|check| check.command.as_str())
                .ne(commands.iter().map(String::as_str))
        {
            let plan = work.plan.clone().unwrap_or_else(|| work.objective.clone());
            work.select(&plan, commands)?;
            self.checkpoint()?;
        }
        let work = self.work.as_ref().ok_or(DriverError::NoWork)?;
        match work.stage() {
            Some(Stage::Plan) if work.policy == Policy::Single && work.plan.is_none() => {
                if let Some(work) = self.work.as_mut() {
                    work.abandon("the plan turn selected no plan");
                }
                self.set_work(objective, Policy::Single)?;
                self.work
                    .as_mut()
                    .ok_or(DriverError::NoWork)?
                    .select(objective, commands)?;
                self.execute()?;
                Ok(false)
            }
            Some(Stage::Plan) => {
                self.execute()?;
                Ok(!created)
            }
            Some(Stage::Execute) => Ok(false),
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
        .is_some_and(|work| {
            std::mem::discriminant(&work.policy) == std::mem::discriminant(&policy)
                && work.objective == text
                && !work.terminal()
        })
}

pub(crate) fn touched_at(path: &Path) -> Vec<String> {
    Work::load(&log::work_path(path))
        .ok()
        .flatten()
        .map(|work| work.touched.into_iter().collect())
        .unwrap_or_default()
}

pub(crate) fn active_at(path: &Path, workspace: &Path) -> Option<(Policy, String)> {
    let Ok(Line::Session {
        workspace: recorded,
        ..
    }) = log::header(path)
    else {
        return None;
    };
    if recorded != canonical(workspace) {
        return None;
    }
    Work::load(&log::work_path(path))
        .ok()
        .flatten()
        .filter(|work| work.policy != Policy::Single && !work.terminal())
        .map(|work| (work.policy, work.objective))
}

pub(crate) fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
