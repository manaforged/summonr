//! Session driver.
//!
//! The driver is the single owner of accepted input. It records the
//! operation-open group atomically in the journal and replays the committed
//! state on reopen. It owns no platform filesystem or process detail; those
//! belong to the publisher and the process supervisor.

mod effects;
mod lifecycle;

use std::path::Path;

use thiserror::Error;

use crate::provider::Image;

use super::bound::{BoundError, Bytes, Items, Text};
use super::context::{
    AbortReason, ContextEntry, ContextEpoch, ContextRole, ContextSource, ImagePayload, InputOrigin,
    InputRoute, OperationClose, OperationOutcome, RequestHead, UserPayload,
};
use super::effect::{
    BudgetKind, Exposure, MeterSettlement, ProviderFailureKind, ProviderIntent, ProviderMessage,
    ProviderOutcome, ProviderPurpose, ToolCall, ToolCancellation, ToolIntent, ToolOutcome,
    ToolResult, Usage,
};
use super::fact::{CommandReceipt, CommandStatus, CompletionCheck};
use super::fact::{Fact, FactData, MeterOwner, OperationStageOutcome};
use super::id::{Digest, OperationId, ProviderResultId, SessionId, ToolCallId};
use super::journal::{JournalError, JournalWriter, Result as JournalResult};
use super::observe::{ObserveError, Observer};
use super::reducer::{InputState, ProviderState, SessionState};
use super::work::{
    AcceptanceCheck, Candidate, EvidenceRef, ItemDisposition, PlanRevision, PlanSource, PlanStep,
    RequiredObservation, VerificationPolicy, WorkError, WorkItem, WorkPolicy, WorkProjection,
    WorkRevision, WorkStage, WorkState, WorkStatus,
};
use super::workspace::WorkspaceObservation;
use super::workspace::{PathComponent, PathKey, WorkspaceScope};
use super::{effect, id, publish};

#[derive(Debug, Error)]
pub(crate) enum DriverError {
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error(transparent)]
    Bound(#[from] BoundError),
    #[error(transparent)]
    Effect(#[from] super::effect::EffectError),
    #[error("operation start requires non-empty text")]
    Empty,
    #[error("no open operation for a provider round")]
    NoOperation,
    #[error("operation has no completed provider round")]
    NoProvider,
    #[error("no planned tool for this call")]
    NoTool,
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Work(#[from] WorkError),
    #[error("session has no Work for Plan selection")]
    NoWork,
    #[error("no workspace root is set for this session")]
    NoWorkspace,
    #[error("session has no context epoch")]
    NoEpoch,
    #[error("journal has no chain head for a fork")]
    NoHead,
    #[error("no open fork intent")]
    NoFork,
    #[error("session is busy")]
    Busy,
    #[error("{0}")]
    Import(String),
    #[error(transparent)]
    Observe(#[from] ObserveError),
    #[error(transparent)]
    Workspace(#[from] super::workspace::WorkspaceError),
}

pub(crate) struct SessionDriver {
    writer: JournalWriter,
    next: u128,
    workspace: Option<std::path::PathBuf>,
    observer: Observer,
}

pub(crate) struct WorkCapture {
    pub(crate) paths: Vec<String>,
    pub(crate) provider: ProviderResultId,
}

pub(crate) struct PreparedVerdict {
    pub(crate) passed: bool,
    pub(crate) diagnostic: Option<String>,
    observation: WorkspaceObservation,
    evidence: Vec<EvidenceRef>,
    synthetic: Option<Fact>,
}

pub(crate) enum VerdictDecision {
    Automatic,
    GoalComplete(String),
    GoalContinue(String),
}

impl SessionDriver {
    /// Open a new session journal, writing the session and epoch roots.
    pub(crate) fn open(
        path: impl AsRef<Path>,
        session: SessionId,
        scope: &WorkspaceScope,
        head: RequestHead,
    ) -> JournalResult<Self> {
        let mut writer = JournalWriter::create(path)?;
        let epoch = super::id::ContextEpochId::new(session, 11);
        let scope_owned = scope.clone();
        writer.append(
            super::id::CommitId::new(session, 1),
            None,
            vec![
                Fact {
                    id: super::id::FactId::new(session, 1),
                    data: FactData::SessionCreated {
                        session,
                        scope: scope_owned,
                    },
                },
                Fact {
                    id: super::id::FactId::new(session, 2),
                    data: FactData::ContextEpochOpened {
                        epoch: ContextEpoch {
                            id: epoch,
                            head: head.clone(),
                            stage: None,
                            base: Items::default(),
                        },
                    },
                },
            ],
        )?;
        Ok(Self::bind(writer, None))
    }

    /// Reopen an existing session journal.
    pub(crate) fn resume(path: impl AsRef<Path>, session: SessionId) -> JournalResult<Self> {
        let driver = Self::bind(JournalWriter::open(path)?, None);
        if driver.session() != session {
            return Err(JournalError::Reduce {
                line: 1,
                source: super::reducer::ReduceError::Namespace {
                    field: "resume session",
                    session,
                },
            });
        }
        Ok(driver)
    }

    /// Open one detached operation for low-level durability tests.
    #[cfg(test)]
    pub(crate) fn start_operation(&mut self, text: &str) -> Result<OperationId, DriverError> {
        self.start_input_with(text, &[], InputOrigin::User, false)
            .map(|(operation, _queued)| operation)
    }

    /// Open one user operation attached to the active Work.
    pub(crate) fn start_input(
        &mut self,
        text: &str,
        images: &[Image],
    ) -> Result<Vec<String>, DriverError> {
        self.start_input_with(text, images, InputOrigin::User, true)
            .map(|(_operation, queued)| queued)
    }

    /// Open the harness-owned Verify operation for the current candidate.
    pub(crate) fn start_verification(&mut self) -> Result<OperationId, DriverError> {
        self.start_input_with(
            "[summon control: verify the captured workspace candidate]",
            &[],
            InputOrigin::WorkPolicy,
            true,
        )
        .map(|(operation, _queued)| operation)
    }

    fn start_input_with(
        &mut self,
        text: &str,
        images: &[Image],
        origin: InputOrigin,
        attach_work: bool,
    ) -> Result<(OperationId, Vec<String>), DriverError> {
        if text.trim().is_empty() {
            return Err(DriverError::Empty);
        }
        if self.writer.state().operation.is_some() {
            return self
                .reject(b"mage:command:start", text.as_bytes(), "busy")
                .map(|operation| (operation, Vec::new()));
        }
        let session = self.session();
        let epoch = self.epoch()?;
        let input = super::id::InputId::new(session, self.take());
        let operation = super::id::OperationId::new(session, self.take());
        let (work, stage) = if attach_work {
            let work = self.work_state()?;
            let stage = work.status().stage().ok_or(DriverError::NoWork)?;
            (Some(work.id()), Some(stage))
        } else {
            (None, None)
        };
        let queued = if origin == InputOrigin::User && stage == Some(WorkStage::Execute) {
            self.writer
                .state()
                .inputs
                .iter()
                .filter_map(|(id, state)| {
                    if !matches!(state, InputState::Pending(InputRoute::Queue)) {
                        return None;
                    }
                    self.writer
                        .state()
                        .input_payloads
                        .get(id)
                        .cloned()
                        .map(|payload| (*id, payload))
                })
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        let entry_id = super::id::ContextEntryId::new(session, self.take());
        let source = if origin == InputOrigin::WorkPolicy {
            ContextSource::WorkPolicy(input)
        } else {
            ContextSource::Input(input)
        };
        let mut entry = ContextEntry {
            id: entry_id,
            epoch,
            role: ContextRole::User,
            source,
            payload: Text::new(text)?,
            digest: Digest::default(),
        };
        entry.digest = entry.calculated_digest();
        let accepted_id = super::id::FactId::new(session, self.take());
        let mut facts = vec![
            Fact {
                id: accepted_id,
                data: FactData::InputAccepted {
                    input,
                    origin,
                    route: InputRoute::Start,
                    payload: UserPayload {
                        text: entry.payload.clone(),
                        images: Items::new(encode_images(images)?)?,
                    },
                },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::InputApplied { input, operation },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::OperationOpened {
                    operation,
                    input,
                    work,
                    stage,
                },
            },
        ];
        let queued_text = queued
            .iter()
            .map(|(_input, payload)| payload.text.as_str().to_string())
            .collect();
        for (queued, payload) in queued {
            let mut queued_entry = ContextEntry {
                id: super::id::ContextEntryId::new(session, self.take()),
                epoch,
                role: ContextRole::User,
                source: ContextSource::WorkPolicy(queued),
                payload: payload.text,
                digest: Digest::default(),
            };
            queued_entry.digest = queued_entry.calculated_digest();
            facts.push(Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::InputApplied {
                    input: queued,
                    operation,
                },
            });
            facts.push(Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ContextEntryAdded {
                    entry: queued_entry,
                },
            });
        }
        facts.push(Fact {
            id: super::id::FactId::new(session, self.take()),
            data: FactData::ContextEntryAdded { entry },
        });
        let command = self.accepted(b"mage:command:start", text.as_bytes(), accepted_id);
        self.append(Some(command), facts)?;
        Ok((operation, queued_text))
    }

    /// Accept steer-routed text against the open operation.
    pub(crate) fn steer(&mut self, text: &str, origin: InputOrigin) -> Result<(), DriverError> {
        if text.trim().is_empty() {
            return Err(DriverError::Empty);
        }
        let operation = self.open_operation()?;
        let session = self.session();
        let epoch = self.epoch()?;
        let input = super::id::InputId::new(session, self.take());
        let mut entry = ContextEntry {
            id: super::id::ContextEntryId::new(session, self.take()),
            epoch,
            role: ContextRole::User,
            source: ContextSource::Input(input),
            payload: Text::new(text)?,
            digest: Digest::default(),
        };
        entry.digest = entry.calculated_digest();
        let accepted_id = super::id::FactId::new(session, self.take());
        let facts = vec![
            Fact {
                id: accepted_id,
                data: FactData::InputAccepted {
                    input,
                    origin,
                    route: InputRoute::Steer,
                    payload: UserPayload {
                        text: entry.payload.clone(),
                        images: Items::default(),
                    },
                },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::InputApplied { input, operation },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ContextEntryAdded { entry },
            },
        ];
        let command = self.accepted(b"mage:command:steer", text.as_bytes(), accepted_id);
        self.append(Some(command), facts)?;
        Ok(())
    }

    /// Open a replacement epoch with a summary while the operation may stay open.
    pub(crate) fn compact(&mut self, summary: &str) -> Result<(), DriverError> {
        let session = self.session();
        let mut head = self.request_head()?.clone();
        head.digest = head.calculated_digest()?;
        let base = self.compact_base()?;
        let epoch = ContextEpoch {
            id: super::id::ContextEpochId::new(session, self.take()),
            head,
            stage: self
                .writer
                .state()
                .operation
                .as_ref()
                .and_then(|operation| operation.stage),
            base: Items::new(base)?,
        };
        let mut entry = ContextEntry {
            id: super::id::ContextEntryId::new(session, self.take()),
            epoch: epoch.id,
            role: ContextRole::System,
            source: ContextSource::HarnessControl,
            payload: Text::new(summary)?,
            digest: Digest::default(),
        };
        entry.digest = entry.calculated_digest();
        let epoch_fact = super::id::FactId::new(session, self.take());
        let facts = vec![
            Fact {
                id: epoch_fact,
                data: FactData::ContextEpochOpened { epoch },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ContextEntryAdded { entry },
            },
        ];
        let command = self.accepted(b"mage:command:compact", summary.as_bytes(), epoch_fact);
        self.append(Some(command), facts)?;
        Ok(())
    }

    /// Replace the idle conversation with an empty epoch.
    pub(crate) fn clear(&mut self) -> Result<(), DriverError> {
        if self.writer.state().operation.is_some() {
            return Err(DriverError::Busy);
        }
        let session = self.session();
        let previous = self.epoch()?;
        let mut head = self.request_head()?.clone();
        head.digest = head.calculated_digest()?;
        let replacement = ContextEpoch {
            id: super::id::ContextEpochId::new(session, self.take()),
            head,
            stage: None,
            base: Items::default(),
        };
        let facts = vec![
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ConversationCleared {
                    previous_epoch: previous,
                    replacement_epoch: replacement.id,
                },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ContextEpochOpened { epoch: replacement },
            },
        ];
        self.append(None, facts)?;
        Ok(())
    }

    fn compact_base(&self) -> Result<Vec<super::id::ContextEntryId>, DriverError> {
        let Some(operation) = self.writer.state().operation.as_ref() else {
            return Ok(Vec::new());
        };
        let input = operation.input;
        let base = self
            .writer
            .state()
            .entries
            .values()
            .find(|entry| entry.source == ContextSource::Input(input))
            .map(|entry| entry.id);
        Ok(base.into_iter().collect())
    }

    fn child_scope(&self, child: SessionId) -> Result<WorkspaceScope, DriverError> {
        let parent = self.scope()?;
        let artifacts = parent
            .artifacts()
            .iter()
            .enumerate()
            .map(|(index, artifact)| super::workspace::ArtifactRoot {
                path: artifact.path.clone(),
                rule: super::id::CoverageRuleId::new(
                    child,
                    u128::try_from(index)
                        .unwrap_or(u128::MAX)
                        .saturating_add(100),
                ),
            })
            .collect();
        Ok(WorkspaceScope::new(
            super::id::WorkspaceScopeId::new(child, 10),
            parent.workspace().clone(),
            parent.candidate_roots().to_vec(),
            parent.reserved().to_vec(),
            artifacts,
        )?)
    }

    /// Close the open operation.
    pub(crate) fn close_operation(&mut self) -> Result<(), DriverError> {
        self.close_with(OperationOutcome::Completed)
    }

    /// Record abort intent, settle open effects, and close as cancelled.
    pub(crate) fn abort(&mut self, reason: AbortReason) -> Result<(), DriverError> {
        self.prepare_abort(reason)?;
        self.close_with(OperationOutcome::Cancelled)
    }

    fn prepare_abort(&mut self, reason: AbortReason) -> Result<(), DriverError> {
        let operation = self.open_operation()?;
        if !self
            .writer
            .state()
            .operation
            .as_ref()
            .is_some_and(|open| open.aborting)
        {
            let session = self.session();
            let fact_id = super::id::FactId::new(session, self.take());
            let command = self.accepted(b"mage:command:abort", b"abort", fact_id);
            self.append(
                Some(command),
                vec![Fact {
                    id: fact_id,
                    data: FactData::AbortRequested { operation, reason },
                }],
            )?;
        }
        if self.writer.state().open_provider.is_some() {
            self.interrupt_open_provider("aborted")?;
        }
        self.cancel_open_tools()?;
        Ok(())
    }

    fn interrupt_open_provider(&mut self, diagnostic: &str) -> Result<(), DriverError> {
        let attempt = self
            .writer
            .state()
            .open_provider
            .ok_or(DriverError::NoOperation)?;
        let result = self.open_provider_result()?;
        let usage = Usage {
            input_tokens: None,
            output_tokens: None,
            cached_input_tokens: None,
            live_millis: None,
        };
        let outcome = ProviderOutcome::Interrupted {
            reason: Text::new(diagnostic)?,
            partial_usage: usage.clone(),
        };
        let facts = self.provider_finish_facts(attempt, result, &[], "", outcome, usage)?;
        self.append(None, facts)?;
        Ok(())
    }

    fn cancel_open_tools(&mut self) -> Result<(), DriverError> {
        let unfinished: Vec<_> = self
            .writer
            .state()
            .tools
            .values()
            .filter(|tool| tool.result.is_none())
            .map(|tool| {
                (
                    tool.call.id,
                    tool.call.result,
                    tool.intent.as_ref().map(|intent| intent.attempt),
                )
            })
            .collect();
        if unfinished.is_empty() {
            return Ok(());
        }
        let session = self.session();
        let epoch = self.epoch()?;
        let mut facts = Vec::new();
        for (call, result, attempt) in &unfinished {
            let outcome = match attempt {
                Some(_) => ToolOutcome::Cancelled {
                    state: ToolCancellation::Stopped,
                },
                None => ToolOutcome::Cancelled {
                    state: ToolCancellation::Prestart,
                },
            };
            facts.push(Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ToolFinished {
                    result: ToolResult {
                        id: *result,
                        call: *call,
                        attempt: *attempt,
                        outcome,
                    },
                },
            });
            if let Some(attempt) = attempt {
                facts.push(Fact {
                    id: super::id::FactId::new(session, self.take()),
                    data: FactData::MeterSettled {
                        owner: MeterOwner::Tool(*attempt),
                        settlement: MeterSettlement::Known(Usage {
                            input_tokens: None,
                            output_tokens: None,
                            cached_input_tokens: None,
                            live_millis: None,
                        }),
                    },
                });
            }
        }
        for (_call, result, _attempt) in &unfinished {
            let mut entry = ContextEntry {
                id: super::id::ContextEntryId::new(session, self.take()),
                epoch,
                role: ContextRole::Tool,
                source: ContextSource::Tool(*result),
                payload: Text::new("aborted")?,
                digest: Digest::default(),
            };
            entry.digest = entry.calculated_digest();
            facts.push(Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ContextEntryAdded { entry },
            });
        }
        self.append(None, facts)?;
        Ok(())
    }

    fn close_with(&mut self, outcome: OperationOutcome) -> Result<(), DriverError> {
        let session = self.session();
        let operation = self.open_operation()?;
        let open = self
            .writer
            .state()
            .operation
            .as_ref()
            .cloned()
            .ok_or(DriverError::NoOperation)?;
        let work = open.work.and_then(|id| self.writer.state().works.get(&id));
        let close = OperationClose {
            id: operation,
            outcome,
            provider: self.latest_provider(operation),
            work: open.work,
            work_status: work.map(|state| state.status().clone()),
            plan: work.and_then(WorkState::selected_plan),
            candidate: work.and_then(|state| state.candidate().map(Candidate::id)),
            verdict: None,
        };
        let closed_id = super::id::FactId::new(session, self.take());
        let facts = vec![Fact {
            id: closed_id,
            data: FactData::OperationClosed {
                close,
                stage_outcome: OperationStageOutcome::Stayed,
            },
        }];
        let command = self.accepted(b"mage:command:close", b"close", closed_id);
        self.append(Some(command), facts)?;
        Ok(())
    }

    fn latest_provider(&self, operation: OperationId) -> Option<ProviderResultId> {
        self.writer
            .state()
            .providers
            .values()
            .filter_map(|provider| {
                let ProviderState::Finished { intent, .. } = provider else {
                    return None;
                };
                (intent.operation == operation)
                    .then_some((intent.attempt.local_u128(), intent.result))
            })
            .max_by_key(|(attempt, _result)| *attempt)
            .map(|(_attempt, result)| result)
    }

    /// Close an open operation left by a previous process.
    pub(crate) fn recover(&mut self) -> Result<(), DriverError> {
        if self.writer.state().operation.is_none() {
            let interrupted_verify = self
                .writer
                .state()
                .works
                .values()
                .any(|work| matches!(work.status(), WorkStatus::Active(WorkStage::Verify)));
            if !interrupted_verify {
                return Ok(());
            }
            self.start_verification()?;
        }
        if self.operation_stage() == Some(WorkStage::Verify) {
            self.prepare_abort(AbortReason::Shutdown)?;
            let diagnostic = "process stopped before the verification verdict";
            let evidence = self.record_completion_check(
                CompletionCheck::Interrupted,
                "interrupted verification",
                false,
                diagnostic,
            )?;
            let mut verdict = self.prepare_verdict(vec![evidence], false)?;
            verdict.diagnostic.get_or_insert(diagnostic.to_string());
            return self.commit_verdict(verdict, VerdictDecision::Automatic, None);
        }
        self.abort(AbortReason::Shutdown)
    }

    /// Bind a real workspace root so the driver can observe it for forks.
    pub(crate) fn set_workspace(&mut self, path: impl AsRef<Path>) {
        let path = path.as_ref().to_path_buf();
        if self.workspace.as_ref() != Some(&path) {
            self.observer = Observer::default();
        }
        self.workspace = Some(path);
    }

    /// Observe the bound workspace as a fresh [`WorkspaceObservation`].
    fn observe_workspace(&mut self) -> Result<WorkspaceObservation, DriverError> {
        let session = self.session();
        let id = super::id::ObservationId::new(session, self.take());
        let workspace = self.workspace.clone().ok_or(DriverError::NoWorkspace)?;
        let scope = self.scope()?.clone();
        self.observer
            .observe(&workspace, &scope, id)
            .map_err(DriverError::Observe)
    }

    /// Open a fork intent from the current clean prefix.
    pub(crate) fn fork(&mut self, child: SessionId) -> Result<super::id::ForkId, DriverError> {
        let session = self.session();
        let intent = super::id::ForkId::new(session, self.take());
        let observation = self.observe_workspace()?;
        let observation_id = observation.id();
        let source_seq = self.writer.state().seq();
        let source_digest = self.writer.state().head().ok_or(DriverError::NoHead)?;
        let planned_fact = super::id::FactId::new(session, self.take());
        let command = CommandReceipt {
            id: super::id::CommandId::new(session, self.take()),
            digest: Digest::hash(b"mage:command:fork", b"fork"),
            status: CommandStatus::Accepted,
            code: None,
            result: Some(planned_fact),
        };
        let facts = vec![
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::WorkspaceObserved { observation },
            },
            Fact {
                id: planned_fact,
                data: FactData::ForkPlanned {
                    intent,
                    child,
                    source_seq,
                    source_digest,
                    observation: observation_id,
                },
            },
        ];
        self.append(Some(command), facts)?;
        Ok(intent)
    }

    /// Publish a forked child journal and close the fork intent on the parent.
    pub(crate) fn publish_fork(
        &mut self,
        intent: super::id::ForkId,
        child_path: &Path,
    ) -> Result<(), DriverError> {
        let open = self
            .writer
            .state()
            .open_fork
            .clone()
            .ok_or(DriverError::NoFork)?;
        if open.id != intent {
            return Err(DriverError::NoFork);
        }
        let parent = self.session();
        let head = self.request_head()?.clone();
        let child = open.child;
        let scope = self.child_scope(child)?;
        let mut child_writer = JournalWriter::create(child_path)?;
        let epoch = ContextEpoch {
            id: super::id::ContextEpochId::new(child, 11),
            head,
            stage: None,
            base: Items::default(),
        };
        child_writer.append(
            super::id::CommitId::new(child, 1),
            None,
            vec![
                Fact {
                    id: super::id::FactId::new(child, 1),
                    data: FactData::SessionCreated {
                        session: child,
                        scope,
                    },
                },
                Fact {
                    id: super::id::FactId::new(child, 2),
                    data: FactData::ForkedFrom {
                        source: parent,
                        source_seq: open.source_seq,
                        source_digest: open.source_digest,
                        intent,
                    },
                },
                Fact {
                    id: super::id::FactId::new(child, 3),
                    data: FactData::ContextEpochOpened { epoch },
                },
            ],
        )?;
        let preceding = child_writer.state().head().ok_or(DriverError::NoHead)?;
        child_writer.append(
            super::id::CommitId::new(child, 2),
            None,
            vec![Fact {
                id: super::id::FactId::new(child, 4),
                data: FactData::ForkSnapshotSealed { intent, preceding },
            }],
        )?;
        let sealed_journal = child_writer.state().head().ok_or(DriverError::NoHead)?;
        drop(child_writer);

        let observation = self.observe_workspace()?;
        let observation_id = observation.id();
        let facts = vec![
            Fact {
                id: super::id::FactId::new(parent, self.take()),
                data: FactData::WorkspaceObserved { observation },
            },
            Fact {
                id: super::id::FactId::new(parent, self.take()),
                data: FactData::ForkFinished {
                    intent,
                    sealed_journal,
                    observation: observation_id,
                },
            },
        ];
        self.append(None, facts)?;
        Ok(())
    }

    /// The committed, reduced session state.
    pub(crate) fn state(&self) -> &SessionState {
        self.writer.state()
    }

    /// Persist session budget ceilings in the journal.
    pub(crate) fn set_budget(
        &mut self,
        tokens: Option<u64>,
        live_millis: Option<u64>,
    ) -> Result<(), DriverError> {
        let requested = [
            (BudgetKind::SessionTokens, tokens),
            (BudgetKind::SessionLiveMillis, live_millis),
        ];
        let changed = requested
            .into_iter()
            .filter(|(kind, limit)| {
                self.writer
                    .state()
                    .meters
                    .limits
                    .get(kind)
                    .copied()
                    .flatten()
                    != *limit
            })
            .collect::<Vec<_>>();
        if changed.is_empty() {
            return Ok(());
        }
        let session = self.session();
        let facts = changed
            .into_iter()
            .map(|(kind, limit)| Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::BudgetChanged { kind, limit },
            })
            .collect::<Vec<_>>();
        let result = facts
            .first()
            .map(|fact| fact.id)
            .ok_or(DriverError::NoWork)?;
        let payload = format!("{tokens:?}:{live_millis:?}");
        let command = self.accepted(b"mage:command:budget", payload.as_bytes(), result);
        self.append(Some(command), facts)
    }

    pub(crate) fn budget_limits(&self) -> (Option<u64>, Option<u64>) {
        let limit = |kind| {
            self.writer
                .state()
                .meters
                .limits
                .get(&kind)
                .copied()
                .flatten()
        };
        (
            limit(BudgetKind::SessionTokens),
            limit(BudgetKind::SessionLiveMillis),
        )
    }

    /// Stage owned by the currently open Work operation, if any.
    pub(crate) fn operation_stage(&self) -> Option<WorkStage> {
        self.writer.state().operation.as_ref()?.stage
    }

    /// Project the reduced session into provider conversation history.
    pub(crate) fn replay(&self) -> crate::session::Replay {
        super::project::replay(self.writer.state())
    }

    /// The journal path this driver owns.
    pub(crate) fn path(&self) -> &Path {
        self.writer.path()
    }

    fn bind(writer: JournalWriter, workspace: Option<std::path::PathBuf>) -> Self {
        Self {
            next: writer.state().next_local().max(21),
            writer,
            workspace,
            observer: Observer::default(),
        }
    }

    fn append(
        &mut self,
        command: Option<CommandReceipt>,
        facts: Vec<Fact>,
    ) -> Result<(), DriverError> {
        let session = self.session();
        let commit = super::id::CommitId::new(session, self.take());
        self.writer
            .append(commit, command, facts)
            .map_err(DriverError::Journal)?;
        self.next = self.next.max(self.writer.state().next_local());
        Ok(())
    }

    fn accepted(
        &mut self,
        namespace: &'static [u8],
        payload: &[u8],
        result: super::id::FactId,
    ) -> CommandReceipt {
        CommandReceipt {
            id: super::id::CommandId::new(self.session(), self.take()),
            digest: Digest::hash(namespace, payload),
            status: CommandStatus::Accepted,
            code: None,
            result: Some(result),
        }
    }

    fn reject(
        &mut self,
        namespace: &'static [u8],
        payload: &[u8],
        code: &str,
    ) -> Result<OperationId, DriverError> {
        let session = self.session();
        let rejected = super::id::FactId::new(session, self.take());
        let command = CommandReceipt {
            id: super::id::CommandId::new(session, self.take()),
            digest: Digest::hash(namespace, payload),
            status: CommandStatus::Rejected,
            code: Some(Text::new(code)?),
            result: Some(rejected),
        };
        self.append(
            Some(command),
            vec![Fact {
                id: rejected,
                data: FactData::CommandRejected {
                    code: Text::new(code)?,
                    diagnostic: Text::new("an operation is already open")?,
                },
            }],
        )?;
        Err(DriverError::Busy)
    }

    pub(crate) fn id(&self) -> super::id::SessionId {
        self.session()
    }

    fn session(&self) -> SessionId {
        self.writer
            .state()
            .session()
            .expect("reduced journal has a session root")
    }

    fn epoch(&self) -> Result<super::id::ContextEpochId, DriverError> {
        self.writer
            .state()
            .current_epoch
            .ok_or(DriverError::NoEpoch)
    }

    fn request_head(&self) -> Result<&RequestHead, DriverError> {
        let epoch = self.epoch()?;
        self.writer
            .state()
            .epochs
            .get(&epoch)
            .map(|epoch| &epoch.head)
            .ok_or(DriverError::NoEpoch)
    }

    fn through(&self) -> Option<super::id::ContextEntryId> {
        let epoch = self.writer.state().current_epoch?;
        self.writer
            .state()
            .epoch_entries
            .get(&epoch)
            .and_then(|entries| entries.last().copied())
    }

    fn current_entries(&self) -> Vec<super::id::ContextEntryId> {
        let Some(epoch) = self.writer.state().current_epoch else {
            return Vec::new();
        };
        self.writer
            .state()
            .epoch_entries
            .get(&epoch)
            .cloned()
            .unwrap_or_default()
    }

    fn open_operation(&self) -> Result<OperationId, DriverError> {
        self.writer
            .state()
            .operation
            .as_ref()
            .map(|operation| operation.id)
            .ok_or(DriverError::NoOperation)
    }

    fn open_provider_result(&self) -> Result<super::id::ProviderResultId, DriverError> {
        let attempt = self
            .writer
            .state()
            .open_provider
            .ok_or(DriverError::NoOperation)?;
        match self.writer.state().providers.get(&attempt) {
            Some(ProviderState::Started(intent) | ProviderState::Finished { intent, .. }) => {
                Ok(intent.result)
            }
            None => Err(DriverError::NoOperation),
        }
    }

    fn work_id(&self) -> Result<super::id::WorkId, DriverError> {
        if let Some(work) = self
            .writer
            .state()
            .operation
            .as_ref()
            .and_then(|operation| operation.work)
        {
            return Ok(work);
        }
        self.writer
            .state()
            .works
            .values()
            .filter(|work| !work.status().terminal())
            .max_by_key(|work| work.id().local_u128())
            .or_else(|| {
                self.writer
                    .state()
                    .works
                    .values()
                    .max_by_key(|work| work.id().local_u128())
            })
            .map(WorkState::id)
            .ok_or(DriverError::NoWork)
    }

    fn work_state(&self) -> Result<WorkState, DriverError> {
        let id = self.work_id()?;
        self.writer
            .state()
            .works
            .get(&id)
            .cloned()
            .ok_or(DriverError::NoWork)
    }

    fn scope(&self) -> Result<&WorkspaceScope, DriverError> {
        let id = self
            .writer
            .state()
            .default_scope
            .ok_or(DriverError::NoWorkspace)?;
        self.writer
            .state()
            .scopes
            .get(&id)
            .ok_or(DriverError::NoWorkspace)
    }

    fn take(&mut self) -> u128 {
        let value = self.next;
        self.next = self.next.saturating_add(1);
        value
    }
}

fn completion_digest(command: &str, passed: bool, output: &str) -> Digest {
    let mut bytes =
        Vec::with_capacity(command.len().saturating_add(output.len()).saturating_add(2));
    bytes.extend_from_slice(command.as_bytes());
    bytes.push(u8::from(passed));
    bytes.extend_from_slice(output.as_bytes());
    Digest::hash(b"mage:completion-check:v1", &bytes)
}

fn path_text(path: &PathKey) -> String {
    path.components()
        .iter()
        .map(|component| match component {
            PathComponent::Unix(bytes) => String::from_utf8_lossy(bytes.as_slice()).into_owned(),
            PathComponent::Windows(units) => String::from_utf16_lossy(units.as_slice()),
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn encode_images(images: &[Image]) -> Result<Vec<ImagePayload>, DriverError> {
    images
        .iter()
        .map(|image| {
            Ok(ImagePayload {
                media_type: Text::new(image.media_type.clone())?,
                data: Bytes::new(image.bytes.clone())?,
            })
        })
        .collect::<Result<Vec<_>, BoundError>>()
        .map_err(DriverError::from)
}
