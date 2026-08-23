//! Session driver.
//!
//! The driver is the single owner of accepted input. It records the
//! operation-open group atomically in the journal and replays the committed
//! state on reopen. It owns no platform filesystem or process detail; those
//! belong to the publisher and the process supervisor.

use std::path::Path;

use thiserror::Error;

use crate::provider::Image;

use super::bound::{BoundError, Bytes, Items, Text};
use super::context::{
    AbortReason, ContextEntry, ContextEpoch, ContextRole, ContextSource, ImagePayload, InputOrigin,
    InputRoute, OperationClose, OperationOutcome, RequestHead, UserPayload,
};
use super::effect::{
    ApprovalSource, BootIdentity, Exposure, JobState as DetachedState, MeterKind, MeterSettlement,
    NativeProcessScope, PolicyDecision, ProcessContainment, ProcessIdentity, ProcessRecord,
    ProcessStart, ProviderFailureKind, ProviderIntent, ProviderMessage, ProviderOutcome,
    ProviderPurpose, ReconciliationContract, ReplayPolicy, StopReason, ToolCall, ToolCancellation,
    ToolIntent, ToolOutcome, ToolResult, Usage, WorkspaceFootprint,
};
use super::fact::{CommandReceipt, CommandStatus, ForkAdoption, ForkOutcome};
use super::fact::{Fact, FactData, MeterOwner, OperationStageOutcome};
use super::fact::{ImportedWorkStatus, LegacySource};
use super::id::{Digest, OperationId, SessionId, ToolCallId};
use super::journal::{JournalError, JournalWriter, Result as JournalResult};
use super::observe::{ObserveError, observe};
use super::reducer::{ProviderState, SessionState};
use super::work::{
    AcceptanceCheck, ItemDisposition, PlanRevision, PlanSource, StageBudget, StageBudgets,
    WorkError, WorkItem, WorkPolicy, WorkProjection, WorkRevision, WorkStage, WorkState,
    WorkStatus,
};
use super::workspace::WorkspaceObservation;
use super::workspace::{PathComponent, PathKey, PathPlatform, WorkspaceEffect, WorkspaceScope};

const MAX_TEXT_BYTES: usize = 65_536;

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

    /// Open a session journal from a validated legacy prefix.
    pub(crate) fn open_legacy(
        path: impl AsRef<Path>,
        session: SessionId,
        scope: &WorkspaceScope,
        head: RequestHead,
        source: LegacySource,
        workspace: &Path,
    ) -> Result<Self, DriverError> {
        let mut writer = JournalWriter::create(path)?;
        let epoch = super::id::ContextEpochId::new(session, 11);
        let scope_owned = scope.clone();
        let observation = observe(workspace, scope, super::id::ObservationId::new(session, 1))
            .map_err(DriverError::Observe)?;
        let observation_id = observation.id();
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
                    data: FactData::LegacyImported {
                        source,
                        plan: None,
                        work: ImportedWorkStatus::None,
                        observation: observation_id,
                        uncertain_tools: Items::default(),
                    },
                },
                Fact {
                    id: super::id::FactId::new(session, 3),
                    data: FactData::WorkspaceObserved { observation },
                },
                Fact {
                    id: super::id::FactId::new(session, 4),
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
        Ok(Self::bind(writer, Some(workspace.to_path_buf())))
    }

    /// Append one imported transcript entry against the current epoch.
    pub(crate) fn import_entry(
        &mut self,
        role: ContextRole,
        text: &str,
    ) -> Result<(), DriverError> {
        let session = self.session();
        let epoch = self.epoch()?;
        let mut entry = ContextEntry {
            id: super::id::ContextEntryId::new(session, self.take()),
            epoch,
            role,
            source: ContextSource::HarnessControl,
            payload: Text::new(text)?,
            digest: Digest::default(),
        };
        entry.digest = entry.calculated_digest();
        let fact_id = super::id::FactId::new(session, self.take());
        self.append(
            None,
            vec![Fact {
                id: fact_id,
                data: FactData::ContextEntryAdded { entry },
            }],
        )?;
        Ok(())
    }

    /// Record one unmatched legacy tool call as uncertain.
    pub(crate) fn import_uncertain(&mut self, provider_call: &str) -> Result<(), DriverError> {
        let session = self.session();
        let epoch = self.epoch()?;
        let result = super::id::ToolResultId::new(session, self.take());
        let mut entry = ContextEntry {
            id: super::id::ContextEntryId::new(session, self.take()),
            epoch,
            role: ContextRole::Tool,
            source: ContextSource::HarnessControl,
            payload: Text::new("legacy tool result unavailable")?,
            digest: Digest::default(),
        };
        entry.digest = entry.calculated_digest();
        let context = entry.id;
        let entry_fact = super::id::FactId::new(session, self.take());
        let uncertain_fact = super::id::FactId::new(session, self.take());
        self.append(
            None,
            vec![
                Fact {
                    id: entry_fact,
                    data: FactData::ContextEntryAdded { entry },
                },
                Fact {
                    id: uncertain_fact,
                    data: FactData::LegacyToolUncertain {
                        provider_call: Text::new(provider_call)?,
                        result,
                        context,
                    },
                },
            ],
        )?;
        Ok(())
    }

    /// Accept a legacy source divergence and anchor this session's imports to it.
    pub(crate) fn accept_legacy(&mut self, source: LegacySource) -> Result<(), DriverError> {
        let session = self.session();
        let accepted_fact = super::id::FactId::new(session, self.take());
        let command = CommandReceipt {
            id: super::id::CommandId::new(session, self.take()),
            digest: Digest::hash(b"mage:command:accept-legacy", b"accept"),
            status: CommandStatus::Accepted,
            code: None,
            result: Some(accepted_fact),
        };
        let facts = vec![Fact {
            id: accepted_fact,
            data: FactData::LegacySourceAccepted { source },
        }];
        self.append(Some(command), facts)?;
        Ok(())
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

    /// Open one operation for a start-routed text input.
    pub(crate) fn start_operation(&mut self, text: &str) -> Result<OperationId, DriverError> {
        self.start_input(text, &[])
    }

    /// Open one operation for start-routed text and images.
    pub(crate) fn start_input(
        &mut self,
        text: &str,
        images: &[Image],
    ) -> Result<OperationId, DriverError> {
        if text.trim().is_empty() {
            return Err(DriverError::Empty);
        }
        if self.writer.state().operation.is_some() {
            return self.reject(b"mage:command:start", text.as_bytes(), "busy");
        }
        let session = self.session();
        let epoch = self.epoch()?;
        let input = super::id::InputId::new(session, self.take());
        let operation = super::id::OperationId::new(session, self.take());
        let entry_id = super::id::ContextEntryId::new(session, self.take());
        let mut entry = ContextEntry {
            id: entry_id,
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
                    origin: InputOrigin::User,
                    route: InputRoute::Start,
                    payload: UserPayload {
                        text: entry.payload.clone(),
                        images: Items::new(encode_images(images)?)?,
                    },
                    reserved_projection_bytes: 128,
                },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::InputApplied {
                    input,
                    operation: Some(operation),
                },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::OperationOpened {
                    operation,
                    input,
                    work: None,
                    stage: None,
                },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ContextEntryAdded { entry },
            },
        ];
        let command = self.accepted(b"mage:command:start", text.as_bytes(), accepted_id);
        self.append(Some(command), facts)?;
        Ok(operation)
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
                    reserved_projection_bytes: 128,
                },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::InputApplied {
                    input,
                    operation: Some(operation),
                },
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
            stage: None,
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

    /// Record a provider attempt against the open operation.
    pub(crate) fn start_provider(
        &mut self,
        request: &[u8],
    ) -> Result<super::id::ProviderAttemptId, DriverError> {
        self.start_provider_as(request, ProviderPurpose::AgentRound)
    }

    /// Record a compaction-summary provider attempt.
    pub(crate) fn start_summary(
        &mut self,
        request: &[u8],
    ) -> Result<super::id::ProviderAttemptId, DriverError> {
        self.start_provider_as(request, ProviderPurpose::CompactionSummary)
    }

    fn start_provider_as(
        &mut self,
        request: &[u8],
        purpose: ProviderPurpose,
    ) -> Result<super::id::ProviderAttemptId, DriverError> {
        let session = self.session();
        let operation = self.open_operation()?;
        let epoch = self.epoch()?;
        let through = self.through();
        let head = self.request_head()?.clone();
        let attempt = super::id::ProviderAttemptId::new(session, self.take());
        let result = super::id::ProviderResultId::new(session, self.take());
        let intent = ProviderIntent {
            attempt,
            result,
            operation,
            epoch,
            purpose,
            work: None,
            plan: None,
            stage: None,
            candidate: None,
            adapter: head.adapter,
            endpoint: head.endpoint,
            model: head.model,
            resource_version: head.resource_version,
            through,
            request_bytes: u32::try_from(request.len()).unwrap_or(u32::MAX),
            request_digest: Digest::hash(b"mage:provider-request:v1", request),
            request_format: head.request_format,
            deadline_unix_millis: 1_800_000_000_000,
            exposure: Exposure {
                max_input_tokens: 1_000,
                max_output_tokens: 1_000,
                max_live_millis: 30_000,
            },
        };
        intent.validate()?;
        let facts = vec![Fact {
            id: super::id::FactId::new(session, self.take()),
            data: FactData::ProviderStarted { intent },
        }];
        self.append(None, facts)?;
        Ok(attempt)
    }

    /// Close a provider round with a completed outcome.
    pub(crate) fn finish_provider(
        &mut self,
        attempt: super::id::ProviderAttemptId,
        text: &str,
        reasoning: &str,
        usage: Usage,
    ) -> Result<(), DriverError> {
        let result = self.open_provider_result()?;
        let outcome = ProviderOutcome::Completed {
            message: ProviderMessage {
                text: Text::new(text)?,
                reasoning: Text::new(reasoning)?,
                tool_calls: Items::default(),
            },
            stop_reason: StopReason::Complete,
            reported_usage: usage.clone(),
        };
        let facts = self.provider_finish_facts(attempt, result, &[], text, outcome, usage)?;
        self.append(None, facts)?;
        Ok(())
    }

    /// Close a provider round and plan one tool obligation.
    /// Returns the planned tool call id.
    pub(crate) fn finish_provider_with_tool(
        &mut self,
        attempt: super::id::ProviderAttemptId,
        text: &str,
        reasoning: &str,
        usage: Usage,
        name: &str,
        arguments: &str,
    ) -> Result<ToolCallId, DriverError> {
        self.finish_provider_with_tools(
            attempt,
            text,
            reasoning,
            usage,
            &[(name, arguments, name)],
        )?
        .into_iter()
        .next()
        .ok_or(DriverError::NoTool)
    }

    /// Close a provider round and plan one tool batch in provider order.
    pub(crate) fn finish_provider_with_tools(
        &mut self,
        attempt: super::id::ProviderAttemptId,
        text: &str,
        reasoning: &str,
        usage: Usage,
        tools: &[(&str, &str, &str)],
    ) -> Result<Vec<ToolCallId>, DriverError> {
        let session = self.session();
        let result = self.open_provider_result()?;
        let planned = self.plan_tools(session, result, tools)?;
        let payloads = planned
            .iter()
            .zip(tools)
            .map(|(tool, (name, arguments, _))| {
                Ok(super::effect::ProviderToolCall {
                    provider_id: tool.provider_id.clone(),
                    name: Text::new(*name)?,
                    arguments: Text::new(*arguments)?,
                })
            })
            .collect::<Result<Vec<_>, BoundError>>()?;
        let outcome = ProviderOutcome::Completed {
            message: ProviderMessage {
                text: Text::new(text)?,
                reasoning: Text::new(reasoning)?,
                tool_calls: Items::new(payloads)?,
            },
            stop_reason: StopReason::ToolCalls,
            reported_usage: usage.clone(),
        };
        let facts = self.provider_finish_facts(attempt, result, &planned, text, outcome, usage)?;
        self.append(None, facts)?;
        Ok(planned.into_iter().map(|tool| tool.id).collect())
    }

    /// Close a provider attempt that did not complete.
    pub(crate) fn fail_provider(
        &mut self,
        attempt: super::id::ProviderAttemptId,
        kind: ProviderFailureKind,
        diagnostic: &str,
    ) -> Result<(), DriverError> {
        let result = self.open_provider_result()?;
        let usage = Usage {
            input_tokens: None,
            output_tokens: None,
            cached_input_tokens: None,
            live_millis: None,
        };
        let outcome = ProviderOutcome::Failed {
            kind,
            diagnostic: Text::new(diagnostic)?,
            partial_usage: usage.clone(),
        };
        let facts = self.provider_finish_facts(attempt, result, &[], "", outcome, usage)?;
        self.append(None, facts)?;
        Ok(())
    }

    fn provider_finish_facts(
        &mut self,
        attempt: super::id::ProviderAttemptId,
        result: super::id::ProviderResultId,
        tools: &[ToolCall],
        text: &str,
        outcome: ProviderOutcome,
        usage: Usage,
    ) -> Result<Vec<Fact>, DriverError> {
        let session = self.session();
        let meter = super::id::MeterId::new(session, self.take());
        let mut entry = ContextEntry {
            id: super::id::ContextEntryId::new(session, self.take()),
            epoch: self.epoch()?,
            role: ContextRole::Assistant,
            source: ContextSource::Provider(result),
            payload: Text::new(text)?,
            digest: Digest::default(),
        };
        entry.digest = entry.calculated_digest();
        let mut facts = vec![
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ProviderFinished {
                    attempt,
                    result,
                    outcome,
                },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::MeterSettled {
                    meter,
                    owner: MeterOwner::Provider(attempt),
                    kind: MeterKind::Provider,
                    settlement: MeterSettlement::Known(usage),
                },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ContextEntryAdded { entry },
            },
        ];
        facts.extend(tools.iter().cloned().map(|call| Fact {
            id: super::id::FactId::new(session, self.take()),
            data: FactData::ToolCallPlanned { call },
        }));
        Ok(facts)
    }

    fn plan_tools(
        &mut self,
        session: SessionId,
        provider_result: super::id::ProviderResultId,
        tools: &[(&str, &str, &str)],
    ) -> Result<Vec<ToolCall>, DriverError> {
        let batch = super::id::ToolBatchId::new(session, self.take());
        let mut planned = Vec::new();
        for (index, (_name, _arguments, provider_id)) in tools.iter().enumerate() {
            let position =
                u16::try_from(index).map_err(|_error| super::effect::EffectError::ToolOrder)?;
            planned.push(ToolCall {
                id: super::id::ToolCallId::new(session, self.take()),
                batch,
                result: super::id::ToolResultId::new(session, self.take()),
                provider_result,
                provider_id: Text::new(*provider_id)?,
                position,
                registration: super::id::ToolRegistrationVersion::new(session, self.take()),
                schema_digest: Digest::hash(b"mage:tool-schema:v1", b"read"),
                effect_contract_digest: ReconciliationContract::None.digest()?,
            });
        }
        Ok(planned)
    }

    /// Record a tool start.
    pub(crate) fn start_tool(
        &mut self,
        call: ToolCallId,
        arguments: &str,
        name: &str,
    ) -> Result<(), DriverError> {
        let session = self.session();
        let registration = self
            .writer
            .state()
            .tools
            .get(&call)
            .ok_or(DriverError::NoTool)?
            .call
            .registration;
        let attempt = super::id::ToolAttemptId::new(session, self.take());
        let platform = self
            .scope()
            .map(WorkspaceScope::platform)
            .unwrap_or(PathPlatform::Unix);
        let footprint = footprint_for(platform, arguments);
        let process = (name == "run_command").then(|| ProcessRecord::Reserved {
            job: super::id::JobId::new(session, self.take()),
            containment: if cfg!(windows) {
                ProcessContainment::WindowsJob
            } else {
                ProcessContainment::PosixGroup
            },
        });
        let intent = ToolIntent {
            attempt,
            call,
            registration,
            normalized_arguments: Text::new(arguments)?,
            decision: PolicyDecision::Allowed,
            approval: ApprovalSource::Policy(Digest::hash(b"mage:policy:v1", b"default")),
            replay: replay_for(name),
            deadline_unix_millis: 1_800_000_000_000,
            reservation: super::id::MeterId::new(session, self.take()),
            footprint,
            reconciliation: ReconciliationContract::None,
            process,
        };
        let facts = vec![Fact {
            id: super::id::FactId::new(session, self.take()),
            data: FactData::ToolStarted { intent },
        }];
        self.append(None, facts)?;
        Ok(())
    }

    /// Record a tool terminal result.
    pub(crate) fn finish_tool(
        &mut self,
        call: ToolCallId,
        output: &str,
        usage: Usage,
    ) -> Result<(), DriverError> {
        self.finish_tools(&[(call, output, usage)])
    }

    /// Record every tool terminal in one batch, then project context in
    /// provider-call order.
    pub(crate) fn finish_tools(
        &mut self,
        results: &[(ToolCallId, &str, Usage)],
    ) -> Result<(), DriverError> {
        if results.is_empty() {
            return Ok(());
        }
        let session = self.session();
        let epoch = self.epoch()?;
        let mut prepared = Vec::new();
        for (call, output, usage) in results {
            let tool = self
                .writer
                .state()
                .tools
                .get(call)
                .ok_or(DriverError::NoTool)?;
            let intent = tool.intent.as_ref().ok_or(DriverError::NoTool)?;
            let attempt = intent.attempt;
            let job = match intent.process.as_ref() {
                Some(ProcessRecord::Reserved { job, .. }) => Some(*job),
                _ => None,
            };
            prepared.push((
                *call,
                tool.call.result,
                attempt,
                *output,
                usage.clone(),
                job,
            ));
        }
        let mut facts = Vec::new();
        for (call, result, attempt, output, usage, reserved) in &prepared {
            let process = reserved.and_then(|job| running_process(job, output));
            let job = process.as_ref().map(ProcessRecord::job);
            facts.push(Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ToolFinished {
                    result: ToolResult {
                        id: *result,
                        call: *call,
                        attempt: Some(*attempt),
                        outcome: ToolOutcome::Completed {
                            output: Text::new(*output)?,
                            job,
                        },
                        workspace: WorkspaceEffect::None,
                    },
                },
            });
            facts.push(Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::MeterSettled {
                    meter: super::id::MeterId::new(session, self.take()),
                    owner: MeterOwner::Tool(*attempt),
                    kind: MeterKind::Tool,
                    settlement: MeterSettlement::Known(usage.clone()),
                },
            });
            if let Some(process) = process {
                facts.push(Fact {
                    id: super::id::FactId::new(session, self.take()),
                    data: FactData::DetachedJobOpened {
                        job: process.job(),
                        attempt: *attempt,
                        process,
                        state: DetachedState::Running,
                        contract_digest: ReconciliationContract::None.digest()?,
                    },
                });
            }
        }
        for (_call, result, _attempt, output, _usage, _reserved) in &prepared {
            let mut entry = ContextEntry {
                id: super::id::ContextEntryId::new(session, self.take()),
                epoch,
                role: ContextRole::Tool,
                source: ContextSource::Tool(*result),
                payload: Text::new(*output)?,
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

    /// Close the open operation.
    pub(crate) fn close_operation(&mut self) -> Result<(), DriverError> {
        self.close_with(OperationOutcome::Completed)
    }

    /// Record abort intent, settle open effects, and close as cancelled.
    pub(crate) fn abort(&mut self, reason: AbortReason) -> Result<(), DriverError> {
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
        self.close_with(OperationOutcome::Cancelled)
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
                        workspace: WorkspaceEffect::None,
                    },
                },
            });
            if let Some(attempt) = attempt {
                facts.push(Fact {
                    id: super::id::FactId::new(session, self.take()),
                    data: FactData::MeterSettled {
                        meter: super::id::MeterId::new(session, self.take()),
                        owner: MeterOwner::Tool(*attempt),
                        kind: MeterKind::Tool,
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
        let close = OperationClose {
            id: operation,
            outcome,
            provider: None,
            work: None,
            work_status: None,
            plan: None,
            candidate: None,
            verdict: None,
            usage_digest: Digest::hash(b"mage:usage-digest:v1", b"closed"),
            inputs: Items::default(),
            diagnostic: None,
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

    /// Close an open operation left by a previous process.
    pub(crate) fn recover(&mut self) -> Result<(), DriverError> {
        if self.writer.state().operation.is_none() {
            return Ok(());
        }
        self.abort(AbortReason::Shutdown)
    }

    /// Set the session Work in the Plan stage.
    pub(crate) fn set_work(&mut self, objective: &str) -> Result<super::id::WorkId, DriverError> {
        let session = self.session();
        let epoch = self.epoch()?;
        let work = super::id::WorkId::new(session, self.take());
        let item = WorkItem {
            id: super::id::WorkItemId::new(session, self.take()),
            disposition: ItemDisposition::Pending,
            title: Text::new("Objective")?,
            evidence: Items::default(),
        };
        let budgets = StageBudgets {
            plan: no_limit(),
            execute: no_limit(),
            verify: no_limit(),
        };
        let entry_id = super::id::ContextEntryId::new(session, self.take());
        let mut entry = ContextEntry {
            id: entry_id,
            epoch,
            role: ContextRole::System,
            source: ContextSource::Work(work),
            payload: Text::new(objective)?,
            digest: Digest::default(),
        };
        entry.digest = entry.calculated_digest();
        let state = WorkState {
            id: work,
            revision: WorkRevision::new(1)?,
            objective: Text::new(objective)?,
            status: WorkStatus::Active(WorkStage::Plan),
            policy: WorkPolicy::Steps,
            selected_plan: None,
            items: Items::new(vec![item])?,
            active_item: None,
            constraints: Items::new(vec![Text::new("One state owner")?])?,
            acceptance_checks: Items::new(vec![super::id::CheckId::new(session, self.take())])?,
            scope: None,
            base: None,
            candidate: None,
            verification: None,
            evidence: Items::default(),
            budgets,
            predecessor: None,
            projection: Text::new("Work projection")?,
            projection_owner: WorkProjection::Context(entry_id),
        };
        state.validate()?;
        let work_fact = super::id::FactId::new(session, self.take());
        let facts = vec![
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ContextEntryAdded { entry },
            },
            Fact {
                id: work_fact,
                data: FactData::WorkSet {
                    work: Box::new(state),
                },
            },
        ];
        let command = self.accepted(b"mage:command:work", objective.as_bytes(), work_fact);
        self.append(Some(command), facts)?;
        Ok(work)
    }

    /// Store a progress snapshot on the active Work projection.
    pub(crate) fn progress(&mut self, snapshot: &str) -> Result<(), DriverError> {
        let mut state = self.work_state()?;
        let session = self.session();
        let epoch = self.epoch()?;
        let entry_id = super::id::ContextEntryId::new(session, self.take());
        let mut entry = ContextEntry {
            id: entry_id,
            epoch,
            role: ContextRole::System,
            source: ContextSource::Work(state.id()),
            payload: Text::new(snapshot)?,
            digest: Digest::default(),
        };
        entry.digest = entry.calculated_digest();
        state.revision = WorkRevision::new(state.revision().get().saturating_add(1))?;
        state.projection = Text::new(snapshot)?;
        state.projection_owner = WorkProjection::Context(entry_id);
        state.validate()?;
        let work_fact = super::id::FactId::new(session, self.take());
        let facts = vec![
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ContextEntryAdded { entry },
            },
            Fact {
                id: work_fact,
                data: FactData::WorkSet {
                    work: Box::new(state),
                },
            },
        ];
        let command = self.accepted(b"mage:command:progress", snapshot.as_bytes(), work_fact);
        self.append(Some(command), facts)?;
        Ok(())
    }

    /// Active Work progress snapshot, when one exists.
    pub(crate) fn progress_text(&self) -> Option<&str> {
        self.writer
            .state()
            .works
            .values()
            .find(|work| matches!(work.status, WorkStatus::Active(_)))
            .map(|work| work.projection.as_str())
    }

    /// Select a supplied Plan revision for the current Work.
    pub(crate) fn select_plan(
        &mut self,
        markdown: &str,
    ) -> Result<super::id::PlanRevisionId, DriverError> {
        let workspace = self.workspace.clone().ok_or(DriverError::NoWorkspace)?;
        let platform = self.scope()?.platform();
        let session = self.session();
        let work = self.work_id()?;
        let revision = super::id::PlanRevisionId::new(session, self.take());
        let attempt = super::id::MaterializationId::new(session, self.take());
        let mut state = self.work_state()?;
        let check_ids = state.acceptance_checks.as_slice().to_vec();
        let checks = check_ids
            .into_iter()
            .map(|id| {
                Ok(AcceptanceCheck {
                    id,
                    command: Text::new("cargo check")?,
                    required_observation: None,
                })
            })
            .collect::<Result<Vec<_>, BoundError>>()?;
        let mut plan = PlanRevision {
            format: 1,
            id: revision,
            work,
            parent: None,
            source: PlanSource::Imported {
                source: Digest::hash(b"mage:plan-source:v1", markdown.as_bytes()),
                converter: 1,
            },
            objective: Text::new(markdown)?,
            constraints: Items::default(),
            steps: Items::default(),
            checks: Items::new(checks)?,
            risks: Items::default(),
            assumptions: Items::default(),
            non_goals: Items::default(),
            markdown: Text::new(markdown)?,
            digest: Digest::default(),
        };
        plan.digest = plan.calculated_digest()?;
        let plan_digest = plan.digest();
        state.revision = WorkRevision::new(state.revision().get().saturating_add(1))?;
        state.selected_plan = Some(revision);
        state.projection_owner = WorkProjection::Plan(revision);
        state.validate()?;
        let footprint = super::publish::footprint_for(platform, session, work, revision, attempt)
            .map_err(|error| DriverError::Work(WorkError::Encoding(error)))?;
        let plan_fact = super::id::FactId::new(session, self.take());
        let facts = vec![
            Fact {
                id: plan_fact,
                data: FactData::PlanSet { plan: plan.clone() },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::PlanMaterializationPlanned {
                    attempt,
                    plan: revision,
                    digest: plan_digest,
                    footprint,
                },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::WorkSet {
                    work: Box::new(state),
                },
            },
        ];
        let command = self.accepted(b"mage:command:plan", markdown.as_bytes(), plan_fact);
        self.append(Some(command), facts)?;
        let outcome = super::publish::materialize(&workspace, platform, session, &plan, attempt);
        let published = outcome.usable();
        let epoch = self.epoch()?;
        let entry_id = super::id::ContextEntryId::new(session, self.take());
        let mut entry = ContextEntry {
            id: entry_id,
            epoch,
            role: ContextRole::System,
            source: ContextSource::Plan(revision),
            payload: Text::new(markdown)?,
            digest: Digest::default(),
        };
        entry.digest = entry.calculated_digest();
        let mut state = self.work_state()?;
        state.revision = WorkRevision::new(state.revision().get().saturating_add(1))?;
        state.projection_owner = WorkProjection::Context(entry_id);
        state.validate()?;
        let facts = vec![
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::PlanMaterializationFinished {
                    attempt,
                    plan: revision,
                    outcome,
                },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::WorkSet {
                    work: Box::new(state),
                },
            },
            Fact {
                id: super::id::FactId::new(session, self.take()),
                data: FactData::ContextEntryAdded { entry },
            },
        ];
        self.append(None, facts)?;
        if !published {
            return Err(DriverError::Import(
                "plan file was not published".to_string(),
            ));
        }
        Ok(revision)
    }

    /// Bind a real workspace root so the driver can observe it for forks.
    pub(crate) fn set_workspace(&mut self, path: impl AsRef<Path>) {
        self.workspace = Some(path.as_ref().to_path_buf());
    }

    /// Observe the bound workspace as a fresh [`WorkspaceObservation`].
    fn observe_workspace(&mut self) -> Result<WorkspaceObservation, DriverError> {
        let session = self.session();
        let id = super::id::ObservationId::new(session, self.take());
        let workspace = self.workspace.clone().ok_or(DriverError::NoWorkspace)?;
        let scope = self.scope()?.clone();
        observe(&workspace, &scope, id).map_err(DriverError::Observe)
    }

    /// Open a fork intent from the current clean prefix.
    pub(crate) fn fork(
        &mut self,
        child: SessionId,
        adoption: ForkAdoption,
    ) -> Result<super::id::ForkId, DriverError> {
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
                    adoption,
                    observation: observation_id,
                    plan_footprint: None,
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
                        inherited_terminal_seq: open.source_seq,
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
                data: FactData::ForkSnapshotSealed {
                    intent,
                    preceding,
                    next_request: None,
                    inherited_terminal_seq: open.source_seq,
                },
            }],
        )?;
        let sealed_journal = child_writer.state().head().ok_or(DriverError::NoHead)?;
        drop(child_writer);

        let observation = self.observe_workspace()?;
        let outcome = ForkOutcome::Published {
            sealed_journal,
            observation: observation.id(),
        };
        let facts = vec![
            Fact {
                id: super::id::FactId::new(parent, self.take()),
                data: FactData::WorkspaceObserved { observation },
            },
            Fact {
                id: super::id::FactId::new(parent, self.take()),
                data: FactData::ForkFinished { intent, outcome },
            },
        ];
        self.append(None, facts)?;
        Ok(())
    }

    /// The committed, reduced session state.
    pub(crate) fn state(&self) -> &SessionState {
        self.writer.state()
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
            .find(|work| matches!(work.status, WorkStatus::Active(_)))
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

fn replay_for(name: &str) -> ReplayPolicy {
    if matches!(name, "edit_file" | "write_file" | "run_command") {
        ReplayPolicy::Never
    } else {
        ReplayPolicy::ReplaySafe
    }
}

fn running_process(job: super::id::JobId, output: &str) -> Option<ProcessRecord> {
    let pid = output.lines().find_map(|line| {
        line.strip_prefix("pid ")?
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()
            .filter(|pid| *pid != 0)
    })?;
    Some(ProcessRecord::Running {
        job,
        identity: ProcessIdentity {
            boot: BootIdentity(Digest::hash(b"mage:boot:v1", b"host")),
            leader: pid,
            started: ProcessStart(1),
            native_scope: if cfg!(windows) {
                NativeProcessScope::WindowsJob(Items::new(vec![pid as u16]).ok()?)
            } else {
                NativeProcessScope::PosixGroup(pid)
            },
        },
    })
}

fn footprint_for(platform: PathPlatform, arguments: &str) -> WorkspaceFootprint {
    let Some(path) = serde_json::from_str::<serde_json::Value>(arguments)
        .ok()
        .and_then(|value| {
            value
                .get("path")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
    else {
        return WorkspaceFootprint::None;
    };
    let Some(key) = path_key(platform, &path) else {
        return WorkspaceFootprint::None;
    };
    Items::new(vec![key])
        .ok()
        .map(WorkspaceFootprint::Exact)
        .unwrap_or(WorkspaceFootprint::None)
}

fn path_key(platform: PathPlatform, relative: &str) -> Option<PathKey> {
    let mut components = Vec::new();
    for part in relative.split(['/', '\\']) {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        let component = match platform {
            PathPlatform::Unix => PathComponent::unix(part.as_bytes()).ok()?,
            PathPlatform::Windows => PathComponent::windows(part.encode_utf16().collect()).ok()?,
        };
        components.push(component);
    }
    PathKey::new(platform, components).ok()
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

fn no_limit() -> StageBudget {
    StageBudget {
        token_limit: None,
        live_millis_limit: None,
        token_reserve: 0,
        live_millis_reserve: 0,
    }
}
