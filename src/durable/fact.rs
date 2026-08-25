use serde::{Deserialize, Serialize};

use super::bound::{Items, Text};
use super::context::{
    AbortReason, ContextEntry, ContextEpoch, InputOrigin, InputRoute, OperationClose, UserPayload,
};
use super::effect::{
    BudgetKind, MeterSettlement, ProviderIntent, ProviderOutcome, ToolCall, ToolIntent, ToolResult,
};
use super::id::{
    CheckId, CommandId, ContextEpochId, Digest, FactId, ForkId, InputId, MaterializationId,
    ObservationId, OperationId, PlanRevisionId, ProviderAttemptId, ProviderResultId, SessionId,
    ToolAttemptId, WorkId,
};
use super::work::{PlanRevision, WorkStage, WorkState};
use super::workspace::{
    PathComponent, PathKey, PathPlatform, WorkspaceError, WorkspaceObservation, WorkspaceScope,
};

const MAX_DIAGNOSTIC_BYTES: usize = 65_536;
const MAX_COMPLETION_CHECK_BYTES: usize = 1_048_576;
const MAX_PUBLICATION_PATHS: usize = 64;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CommandStatus {
    Accepted,
    Rejected,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct CommandReceipt {
    pub(crate) id: CommandId,
    pub(crate) digest: Digest,
    pub(crate) status: CommandStatus,
    pub(crate) code: Option<Text<256>>,
    pub(crate) result: Option<FactId>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct PublicationFootprint {
    pub(crate) ancestors: Items<PathKey, MAX_PUBLICATION_PATHS>,
    pub(crate) temporary: PathKey,
    pub(crate) destination: PathKey,
}

impl PublicationFootprint {
    pub(crate) fn validate(&self, attempt: &str) -> Result<(), WorkspaceError> {
        let Some(parent) = self.destination.parent() else {
            return Err(WorkspaceError::Component);
        };
        if self.temporary.parent().as_ref() != Some(&parent) || self.temporary == self.destination {
            return Err(WorkspaceError::Overlap);
        }
        let expected = format!(".mage-{attempt}.tmp");
        let expected = match self.destination.platform() {
            PathPlatform::Unix => PathComponent::unix(expected.into_bytes())?,
            PathPlatform::Windows => PathComponent::windows(expected.encode_utf16().collect())?,
        };
        if self.temporary.last() != Some(&expected) {
            return Err(WorkspaceError::Component);
        }
        let mut length = 0;
        for ancestor in self.ancestors.iter() {
            if ancestor.platform() != self.destination.platform()
                || !ancestor.prefixes(&self.destination)
                || ancestor.len() <= length
            {
                return Err(WorkspaceError::Order);
            }
            length = ancestor.len();
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", content = "data", rename_all = "snake_case")]
pub(crate) enum MaterializationOutcome {
    Written,
    Matched,
    Conflict {
        observed: Digest,
    },
    Failed {
        diagnostic: Text<MAX_DIAGNOSTIC_BYTES>,
    },
}

impl MaterializationOutcome {
    pub(crate) fn usable(&self) -> bool {
        matches!(self, Self::Written | Self::Matched)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OperationStageOutcome {
    Stayed,
    Advanced,
    Returned,
    Terminal,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "owner", content = "id", rename_all = "snake_case")]
pub(crate) enum MeterOwner {
    Provider(ProviderAttemptId),
    Tool(ToolAttemptId),
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "source", content = "id")]
pub(crate) enum CompletionCheck {
    Declared(CheckId),
    CandidateStable,
    Interrupted,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Fact {
    pub(crate) id: FactId,
    #[serde(flatten)]
    pub(crate) data: FactData,
}

impl Fact {
    pub(crate) fn kind(&self) -> FactKind {
        self.data.kind()
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "fact", content = "data", rename_all = "snake_case")]
pub(crate) enum FactData {
    SessionCreated {
        session: SessionId,
        scope: WorkspaceScope,
    },
    BudgetChanged {
        kind: BudgetKind,
        limit: Option<u64>,
    },
    PlanSet {
        plan: PlanRevision,
    },
    PlanMaterializationPlanned {
        attempt: MaterializationId,
        plan: PlanRevisionId,
        digest: Digest,
        footprint: PublicationFootprint,
    },
    PlanMaterializationFinished {
        attempt: MaterializationId,
        plan: PlanRevisionId,
        outcome: MaterializationOutcome,
    },
    ForkPlanned {
        intent: ForkId,
        child: SessionId,
        source_seq: u64,
        source_digest: Digest,
        observation: ObservationId,
    },
    ForkFinished {
        intent: ForkId,
        sealed_journal: Digest,
        observation: ObservationId,
    },
    ForkedFrom {
        source: SessionId,
        source_seq: u64,
        source_digest: Digest,
        intent: ForkId,
    },
    ForkSnapshotSealed {
        intent: ForkId,
        preceding: Digest,
    },
    CommandRejected {
        code: Text<256>,
        diagnostic: Text<MAX_DIAGNOSTIC_BYTES>,
    },
    InputAccepted {
        input: InputId,
        origin: InputOrigin,
        route: InputRoute,
        payload: UserPayload,
    },
    InputApplied {
        input: InputId,
        operation: OperationId,
    },
    WorkSet {
        work: Box<WorkState>,
    },
    WorkspaceObserved {
        observation: WorkspaceObservation,
    },
    OperationOpened {
        operation: OperationId,
        input: InputId,
        work: Option<WorkId>,
        stage: Option<WorkStage>,
    },
    AbortRequested {
        operation: OperationId,
        reason: AbortReason,
    },
    OperationClosed {
        close: OperationClose,
        stage_outcome: OperationStageOutcome,
    },
    ProviderStarted {
        intent: ProviderIntent,
    },
    ProviderFinished {
        attempt: ProviderAttemptId,
        result: ProviderResultId,
        outcome: ProviderOutcome,
    },
    CompletionCheckRecorded {
        operation: OperationId,
        check: CompletionCheck,
        command: Text<MAX_COMPLETION_CHECK_BYTES>,
        passed: bool,
        output: Text<MAX_COMPLETION_CHECK_BYTES>,
    },
    ToolCallPlanned {
        call: ToolCall,
    },
    ToolStarted {
        intent: ToolIntent,
    },
    ToolFinished {
        result: ToolResult,
    },
    ContextEpochOpened {
        epoch: ContextEpoch,
    },
    ContextEntryAdded {
        entry: ContextEntry,
    },
    ConversationCleared {
        previous_epoch: ContextEpochId,
        replacement_epoch: ContextEpochId,
    },
    MeterSettled {
        owner: MeterOwner,
        settlement: MeterSettlement,
    },
}

impl FactData {
    pub(crate) fn kind(&self) -> FactKind {
        match self {
            Self::SessionCreated { .. } => FactKind::SessionCreated,
            Self::BudgetChanged { .. } => FactKind::BudgetChanged,
            Self::PlanSet { .. } => FactKind::PlanSet,
            Self::PlanMaterializationPlanned { .. } => FactKind::PlanMaterializationPlanned,
            Self::PlanMaterializationFinished { .. } => FactKind::PlanMaterializationFinished,
            Self::ForkPlanned { .. } => FactKind::ForkPlanned,
            Self::ForkFinished { .. } => FactKind::ForkFinished,
            Self::ForkedFrom { .. } => FactKind::ForkedFrom,
            Self::ForkSnapshotSealed { .. } => FactKind::ForkSnapshotSealed,
            Self::CommandRejected { .. } => FactKind::CommandRejected,
            Self::InputAccepted { .. } => FactKind::InputAccepted,
            Self::InputApplied { .. } => FactKind::InputApplied,
            Self::WorkSet { .. } => FactKind::WorkSet,
            Self::WorkspaceObserved { .. } => FactKind::WorkspaceObserved,
            Self::OperationOpened { .. } => FactKind::OperationOpened,
            Self::AbortRequested { .. } => FactKind::AbortRequested,
            Self::OperationClosed { .. } => FactKind::OperationClosed,
            Self::ProviderStarted { .. } => FactKind::ProviderStarted,
            Self::ProviderFinished { .. } => FactKind::ProviderFinished,
            Self::CompletionCheckRecorded { .. } => FactKind::CompletionCheckRecorded,
            Self::ToolCallPlanned { .. } => FactKind::ToolCallPlanned,
            Self::ToolStarted { .. } => FactKind::ToolStarted,
            Self::ToolFinished { .. } => FactKind::ToolFinished,
            Self::ContextEpochOpened { .. } => FactKind::ContextEpochOpened,
            Self::ContextEntryAdded { .. } => FactKind::ContextEntryAdded,
            Self::ConversationCleared { .. } => FactKind::ConversationCleared,
            Self::MeterSettled { .. } => FactKind::MeterSettled,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FactKind {
    SessionCreated,
    BudgetChanged,
    PlanSet,
    PlanMaterializationPlanned,
    PlanMaterializationFinished,
    ForkPlanned,
    ForkFinished,
    ForkedFrom,
    ForkSnapshotSealed,
    CommandRejected,
    InputAccepted,
    InputApplied,
    WorkSet,
    WorkspaceObserved,
    OperationOpened,
    AbortRequested,
    OperationClosed,
    ProviderStarted,
    ProviderFinished,
    CompletionCheckRecorded,
    ToolCallPlanned,
    ToolStarted,
    ToolFinished,
    ContextEpochOpened,
    ContextEntryAdded,
    ConversationCleared,
    MeterSettled,
}

impl FactKind {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 27] = [
        Self::SessionCreated,
        Self::BudgetChanged,
        Self::PlanSet,
        Self::PlanMaterializationPlanned,
        Self::PlanMaterializationFinished,
        Self::ForkPlanned,
        Self::ForkFinished,
        Self::ForkedFrom,
        Self::ForkSnapshotSealed,
        Self::CommandRejected,
        Self::InputAccepted,
        Self::InputApplied,
        Self::WorkSet,
        Self::WorkspaceObserved,
        Self::OperationOpened,
        Self::AbortRequested,
        Self::OperationClosed,
        Self::ProviderStarted,
        Self::ProviderFinished,
        Self::CompletionCheckRecorded,
        Self::ToolCallPlanned,
        Self::ToolStarted,
        Self::ToolFinished,
        Self::ContextEpochOpened,
        Self::ContextEntryAdded,
        Self::ConversationCleared,
        Self::MeterSettled,
    ];
}
