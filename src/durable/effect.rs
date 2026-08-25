use serde::{Deserialize, Serialize};
use serde_json::{Error as JsonError, to_vec};
use thiserror::Error;

use super::bound::{Items, Text};
use super::id::{
    CandidateId, CommandId, ContextEntryId, ContextEpochId, Digest, EvidenceId, JobId, MeterId,
    ObservationId, OperationId, PlanRevisionId, ProviderAttemptId, ProviderResultId,
    ReconciliationRuleId, ToolAttemptId, ToolBatchId, ToolCallId, ToolRegistrationVersion,
    ToolResultId, WorkId, WorkspaceScopeId,
};
use super::work::WorkStage;
use super::workspace::{PathKey, WorkspaceEffect};

const MAX_TEXT_BYTES: usize = 1_048_576;
const MAX_DIAGNOSTIC_BYTES: usize = 65_536;
const MAX_TOOL_CALLS: usize = 128;
const MAX_TOOL_PATHS: usize = 512;
const MAX_TOOL_ARGUMENT_BYTES: usize = 1_048_576;
const MAX_PROCESS_NAME_UNITS: usize = 512;
const MAX_REQUEST_BYTES: u32 = 16 * 1_048_576;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum EffectError {
    #[error("process identity contains a zero leader or start marker")]
    ProcessIdentity,
    #[error("process identity does not match its selected containment")]
    ProcessContainment,
    #[error("provider request size must be between 1 and {MAX_REQUEST_BYTES} bytes")]
    RequestSize,
    #[error("tool call positions must be strictly increasing")]
    ToolOrder,
    #[error("tool call or result identity is duplicated in one batch")]
    ToolIdentity,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderPurpose {
    AgentRound,
    CompactionSummary,
    GoalJudge,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Exposure {
    pub(crate) max_input_tokens: u64,
    pub(crate) max_output_tokens: u64,
    pub(crate) max_live_millis: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ProviderIntent {
    pub(crate) attempt: ProviderAttemptId,
    pub(crate) result: ProviderResultId,
    pub(crate) operation: OperationId,
    pub(crate) epoch: ContextEpochId,
    pub(crate) purpose: ProviderPurpose,
    pub(crate) work: Option<WorkId>,
    pub(crate) plan: Option<PlanRevisionId>,
    pub(crate) stage: Option<WorkStage>,
    pub(crate) candidate: Option<CandidateId>,
    pub(crate) adapter: Text<256>,
    pub(crate) endpoint: Text<2_048>,
    pub(crate) model: Text<1_024>,
    pub(crate) resource_version: Option<Text<1_024>>,
    pub(crate) through: Option<ContextEntryId>,
    pub(crate) request_bytes: u32,
    pub(crate) request_digest: Digest,
    pub(crate) request_format: u16,
    pub(crate) deadline_unix_millis: u64,
    pub(crate) exposure: Exposure,
}

impl ProviderIntent {
    pub(crate) fn validate(&self) -> Result<(), EffectError> {
        if self.request_bytes == 0 || self.request_bytes > MAX_REQUEST_BYTES {
            return Err(EffectError::RequestSize);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Usage {
    pub(crate) input_tokens: Option<u64>,
    pub(crate) output_tokens: Option<u64>,
    pub(crate) cached_input_tokens: Option<u64>,
    pub(crate) live_millis: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ProviderToolCall {
    pub(crate) provider_id: Text<1_024>,
    pub(crate) name: Text<1_024>,
    pub(crate) arguments: Text<MAX_TOOL_ARGUMENT_BYTES>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ProviderMessage {
    pub(crate) text: Text<MAX_TEXT_BYTES>,
    pub(crate) reasoning: Text<MAX_TEXT_BYTES>,
    pub(crate) tool_calls: Items<ProviderToolCall, MAX_TOOL_CALLS>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "reason", content = "detail", rename_all = "snake_case")]
pub(crate) enum StopReason {
    Complete,
    ToolCalls,
    Length,
    Refusal,
    Other(Text<1_024>),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
pub(crate) enum ProviderOutcome {
    Completed {
        message: ProviderMessage,
        stop_reason: StopReason,
        reported_usage: Usage,
    },
    Failed {
        kind: ProviderFailureKind,
        diagnostic: Text<MAX_DIAGNOSTIC_BYTES>,
        partial_usage: Usage,
    },
    Interrupted {
        reason: Text<MAX_DIAGNOSTIC_BYTES>,
        partial_usage: Usage,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProviderFailureKind {
    Transport,
    RateLimit,
    Authentication,
    ContextOverflow,
    InvalidRequest,
    ResponseTooLarge,
    UnsupportedRequestBasis,
    Other,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReplayPolicy {
    Never,
    ReplaySafe,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "contract", content = "data", rename_all = "snake_case")]
pub(crate) enum ReconciliationContract {
    None,
    Process {
        job: JobId,
    },
    External {
        observer: ToolRegistrationVersion,
        rule: ReconciliationRuleId,
        subject: Digest,
    },
    Unobservable,
}

impl ReconciliationContract {
    pub(crate) fn digest(&self) -> Result<Digest, JsonError> {
        to_vec(self).map(|bytes| Digest::hash(b"mage:reconciliation-contract:v1", &bytes))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "footprint", content = "data", rename_all = "snake_case")]
pub(crate) enum WorkspaceFootprint {
    None,
    Exact(Items<PathKey, MAX_TOOL_PATHS>),
    Scope(WorkspaceScopeId),
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PolicyDecision {
    Allowed,
    Approved,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "source", content = "data", rename_all = "snake_case")]
pub(crate) enum ApprovalSource {
    Policy(Digest),
    User(CommandId),
    Harness(Digest),
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ProcessContainment {
    PosixGroup,
    LinuxNamespace,
    WindowsJob,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "scope", content = "identity", rename_all = "snake_case")]
pub(crate) enum NativeProcessScope {
    PosixGroup(u64),
    LinuxNamespace { group: u64, namespace: u64 },
    WindowsJob(Items<u16, MAX_PROCESS_NAME_UNITS>),
}

impl NativeProcessScope {
    fn containment(&self) -> ProcessContainment {
        match self {
            Self::PosixGroup(_) => ProcessContainment::PosixGroup,
            Self::LinuxNamespace { .. } => ProcessContainment::LinuxNamespace,
            Self::WindowsJob(_) => ProcessContainment::WindowsJob,
        }
    }

    fn valid(&self) -> bool {
        match self {
            Self::PosixGroup(group) => *group != 0,
            Self::LinuxNamespace { group, namespace } => *group != 0 && *namespace != 0,
            Self::WindowsJob(name) => !name.is_empty() && name.iter().all(|unit| *unit != 0),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct BootIdentity(pub(crate) Digest);

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ProcessStart(pub(crate) u64);

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ProcessIdentity {
    pub(crate) boot: BootIdentity,
    pub(crate) leader: u64,
    pub(crate) started: ProcessStart,
    pub(crate) native_scope: NativeProcessScope,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ProcessExit {
    pub(crate) code: Option<i32>,
    pub(crate) signal: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "data", rename_all = "snake_case")]
pub(crate) enum ProcessRecord {
    Reserved {
        job: JobId,
        containment: ProcessContainment,
    },
    Running {
        job: JobId,
        identity: ProcessIdentity,
    },
    Terminal {
        job: JobId,
        identity: ProcessIdentity,
        exit: ProcessExit,
    },
}

impl ProcessRecord {
    pub(crate) fn job(&self) -> JobId {
        match self {
            Self::Reserved { job, .. } | Self::Running { job, .. } | Self::Terminal { job, .. } => {
                *job
            }
        }
    }

    pub(crate) fn validate(&self, selected: ProcessContainment) -> Result<(), EffectError> {
        match self {
            Self::Reserved { containment, .. } if *containment == selected => Ok(()),
            Self::Reserved { .. } => Err(EffectError::ProcessContainment),
            Self::Running { identity, .. } | Self::Terminal { identity, .. } => {
                if identity.leader == 0 || identity.started.0 == 0 || !identity.native_scope.valid()
                {
                    return Err(EffectError::ProcessIdentity);
                }
                if identity.native_scope.containment() != selected {
                    return Err(EffectError::ProcessContainment);
                }
                Ok(())
            }
        }
    }

    pub(crate) fn containment(&self) -> ProcessContainment {
        match self {
            Self::Reserved { containment, .. } => *containment,
            Self::Running { identity, .. } | Self::Terminal { identity, .. } => {
                identity.native_scope.containment()
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ToolCall {
    pub(crate) id: ToolCallId,
    pub(crate) batch: ToolBatchId,
    pub(crate) result: ToolResultId,
    pub(crate) provider_result: ProviderResultId,
    pub(crate) provider_id: Text<1_024>,
    pub(crate) position: u16,
    pub(crate) registration: ToolRegistrationVersion,
    pub(crate) schema_digest: Digest,
    pub(crate) effect_contract_digest: Digest,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ToolIntent {
    pub(crate) attempt: ToolAttemptId,
    pub(crate) call: ToolCallId,
    pub(crate) registration: ToolRegistrationVersion,
    pub(crate) normalized_arguments: Text<MAX_TOOL_ARGUMENT_BYTES>,
    pub(crate) decision: PolicyDecision,
    pub(crate) approval: ApprovalSource,
    pub(crate) replay: ReplayPolicy,
    pub(crate) deadline_unix_millis: u64,
    pub(crate) reservation: MeterId,
    pub(crate) footprint: WorkspaceFootprint,
    pub(crate) reconciliation: ReconciliationContract,
    pub(crate) process: Option<ProcessRecord>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
pub(crate) enum ToolOutcome {
    Completed {
        output: Text<MAX_TEXT_BYTES>,
        job: Option<JobId>,
    },
    Failed {
        diagnostic: Text<MAX_DIAGNOSTIC_BYTES>,
    },
    Denied {
        diagnostic: Text<MAX_DIAGNOSTIC_BYTES>,
    },
    Unavailable {
        diagnostic: Text<MAX_DIAGNOSTIC_BYTES>,
    },
    Cancelled {
        state: ToolCancellation,
    },
    Uncertain {
        diagnostic: Text<MAX_DIAGNOSTIC_BYTES>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ToolCancellation {
    Prestart,
    Stopped,
}

impl ToolOutcome {
    pub(crate) fn needs_start(&self) -> bool {
        !matches!(
            self,
            Self::Denied { .. }
                | Self::Unavailable { .. }
                | Self::Cancelled {
                    state: ToolCancellation::Prestart
                }
        )
    }

    pub(crate) fn uncertain(&self) -> bool {
        matches!(self, Self::Uncertain { .. })
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ToolResult {
    pub(crate) id: ToolResultId,
    pub(crate) call: ToolCallId,
    pub(crate) attempt: Option<ToolAttemptId>,
    pub(crate) outcome: ToolOutcome,
    pub(crate) workspace: WorkspaceEffect,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DetachedState {
    Running,
    Uncertain,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReconciliationResolution {
    Open,
    TerminalKnown,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "evidence", content = "data", rename_all = "snake_case")]
pub(crate) enum ReconciliationEvidence {
    Process {
        job: JobId,
        record: ProcessRecord,
    },
    External {
        observer: ToolRegistrationVersion,
        rule: ReconciliationRuleId,
        subject: Digest,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct EffectObservation {
    pub(crate) target: ToolAttemptId,
    pub(crate) contract_digest: Digest,
    pub(crate) basis: ReconciliationEvidence,
    pub(crate) resolution: ReconciliationResolution,
    pub(crate) evidence: EvidenceId,
    pub(crate) workspace: Option<ObservationId>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DetachedClose {
    Terminal,
    AbandonedUnknown,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MeterKind {
    Provider,
    Tool,
    Retry,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "settlement", content = "data", rename_all = "snake_case")]
pub(crate) enum MeterSettlement {
    Known(Usage),
    Conservative(Exposure),
    Unknown(Exposure),
}

#[derive(Clone, Copy, Debug, Deserialize, Hash, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BudgetKind {
    SessionTokens,
    SessionLiveMillis,
    PlanTokens,
    ExecuteTokens,
    VerifyTokens,
    PlanLiveMillis,
    ExecuteLiveMillis,
    VerifyLiveMillis,
}

pub(crate) fn validate_calls(calls: &[&ToolCall]) -> Result<(), EffectError> {
    if calls
        .windows(2)
        .any(|pair| pair[0].position >= pair[1].position)
    {
        return Err(EffectError::ToolOrder);
    }
    for (index, call) in calls.iter().enumerate() {
        if calls[..index]
            .iter()
            .any(|prior| prior.provider_id == call.provider_id || prior.result == call.result)
        {
            return Err(EffectError::ToolIdentity);
        }
    }
    Ok(())
}
