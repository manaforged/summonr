use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::to_vec;
use thiserror::Error;

use super::bound::{BoundError, Items, Text};
use super::id::{
    CandidateId, CheckId, ContextEntryId, ContextEpochId, CoverageRuleId, Digest, EvidenceId,
    FactId, InputId, ObservationId, OperationId, PlanRevisionId, ProviderResultId, SessionId,
    ToolBatchId, VerificationPolicyId, WorkId, WorkItemId, WorkspaceScopeId,
};
use super::workspace::{
    ArtifactRoot, PathComponent, PathKey, PathPlatform, WorkspaceError, WorkspaceScope,
};

const MAX_OBJECTIVE_BYTES: usize = 65_536;
const MAX_TEXT_BYTES: usize = 65_536;
const MAX_MARKDOWN_BYTES: usize = 1_048_576;
const MAX_PROJECTION_BYTES: usize = 262_144;
const MAX_ITEMS: usize = 512;
const MAX_DEPENDENCIES: usize = 64;
const MAX_CHECKS: usize = 256;
const MAX_EVIDENCE: usize = 2_048;
const MAX_RULES: usize = 256;
const MAX_ARTIFACTS: usize = 64;

#[derive(Clone, Debug, Error, PartialEq, Eq)]
pub(crate) enum WorkError {
    #[error(transparent)]
    Bound(#[from] BoundError),
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
    #[error("plan encoding failed: {0}")]
    Encoding(String),
    #[error("plan digest does not match its canonical body")]
    PlanDigest,
    #[error("candidate digest does not match its canonical body")]
    CandidateDigest,
    #[error("work revision must be greater than zero")]
    Revision,
    #[error("active item is not present in the Work item set")]
    ActiveItem,
    #[error("terminal Work cannot have an active item")]
    TerminalActiveItem,
    #[error("Execute and Verify require a selected Plan and verification policy")]
    Selection,
    #[error("Execute and Verify require a workspace scope and base observation")]
    Base,
    #[error("Verify and completed Work require a candidate")]
    Candidate,
    #[error("Work item IDs must be unique")]
    ItemOrder,
    #[error("Plan step dependencies must refer to another step in the revision")]
    Dependency,
    #[error("Plan check IDs and step dependencies must be unique")]
    PlanIdentity,
    #[error("Plan format is unsupported")]
    PlanFormat,
    #[error("Work candidate does not match its selected Plan, policy, scope, or base")]
    CandidateBinding,
    #[error("Work candidate manifest does not match its base and capture")]
    CandidateManifest,
    #[error("stage budget reserve exceeds its configured limit")]
    StageBudget,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub(crate) struct WorkRevision(u64);

impl WorkRevision {
    pub(crate) fn new(value: u64) -> Result<Self, WorkError> {
        if value == 0 {
            return Err(WorkError::Revision);
        }
        Ok(Self(value))
    }

    pub(crate) fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkStage {
    Plan,
    Execute,
    Verify,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "code", content = "detail", rename_all = "snake_case")]
pub(crate) enum PauseReason {
    User,
    Budget,
    Materialization,
    UnknownEffect,
    VerificationLimit,
    Other(Text<MAX_TEXT_BYTES>),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "code", content = "detail", rename_all = "snake_case")]
pub(crate) enum CloseReason {
    User,
    Superseded,
    UnrecoverableEffect,
    Other(Text<MAX_TEXT_BYTES>),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", content = "detail", rename_all = "snake_case")]
pub(crate) enum LegacyWorkOutcome {
    Completed,
    Failed,
    Cancelled,
    Unknown(Text<MAX_TEXT_BYTES>),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "status", content = "data", rename_all = "snake_case")]
pub(crate) enum WorkStatus {
    Active(WorkStage),
    Paused {
        stage: WorkStage,
        reason: PauseReason,
    },
    Completed {
        candidate: CandidateId,
        verdict: EvidenceId,
    },
    Abandoned {
        reason: CloseReason,
    },
    LegacyTerminal {
        outcome: LegacyWorkOutcome,
    },
}

impl WorkStatus {
    pub(crate) fn stage(&self) -> Option<WorkStage> {
        match self {
            Self::Active(stage) | Self::Paused { stage, .. } => Some(*stage),
            Self::Completed { .. } | Self::Abandoned { .. } | Self::LegacyTerminal { .. } => None,
        }
    }

    pub(crate) fn terminal(&self) -> bool {
        self.stage().is_none()
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkPolicy {
    Single,
    Steps,
    Goal,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ItemDisposition {
    Pending,
    Active,
    Completed,
    Blocked,
    Retired,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct EvidenceRef {
    pub(crate) id: EvidenceId,
    pub(crate) source: FactId,
    pub(crate) digest: Digest,
    pub(crate) summary: Text<MAX_TEXT_BYTES>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct WorkItem {
    pub(crate) id: WorkItemId,
    pub(crate) disposition: ItemDisposition,
    pub(crate) title: Text<MAX_TEXT_BYTES>,
    pub(crate) evidence: Items<EvidenceId, MAX_EVIDENCE>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct StageBudget {
    pub(crate) token_limit: Option<u64>,
    pub(crate) live_millis_limit: Option<u64>,
    pub(crate) token_reserve: u64,
    pub(crate) live_millis_reserve: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct StageBudgets {
    pub(crate) plan: StageBudget,
    pub(crate) execute: StageBudget,
    pub(crate) verify: StageBudget,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "source", content = "data", rename_all = "snake_case")]
pub(crate) enum PlanSource {
    Generated { provider: ProviderResultId },
    Supplied { input: InputId },
    Imported { source: Digest, converter: u16 },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct PlanStep {
    pub(crate) id: WorkItemId,
    pub(crate) title: Text<MAX_TEXT_BYTES>,
    pub(crate) detail: Text<MAX_TEXT_BYTES>,
    pub(crate) dependencies: Items<WorkItemId, MAX_DEPENDENCIES>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct AcceptanceCheck {
    pub(crate) id: CheckId,
    pub(crate) command: Text<MAX_TEXT_BYTES>,
    pub(crate) required_observation: Option<Text<MAX_TEXT_BYTES>>,
}

#[derive(Serialize)]
struct PlanBody<'a> {
    format: u16,
    id: PlanRevisionId,
    work: WorkId,
    parent: Option<PlanRevisionId>,
    source: &'a PlanSource,
    objective: &'a Text<MAX_OBJECTIVE_BYTES>,
    constraints: &'a Items<Text<MAX_TEXT_BYTES>, MAX_RULES>,
    steps: &'a Items<PlanStep, MAX_ITEMS>,
    checks: &'a Items<AcceptanceCheck, MAX_CHECKS>,
    risks: &'a Items<Text<MAX_TEXT_BYTES>, MAX_RULES>,
    assumptions: &'a Items<Text<MAX_TEXT_BYTES>, MAX_RULES>,
    non_goals: &'a Items<Text<MAX_TEXT_BYTES>, MAX_RULES>,
    markdown: &'a Text<MAX_MARKDOWN_BYTES>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct PlanRevision {
    pub(crate) format: u16,
    pub(crate) id: PlanRevisionId,
    pub(crate) work: WorkId,
    pub(crate) parent: Option<PlanRevisionId>,
    pub(crate) source: PlanSource,
    pub(crate) objective: Text<MAX_OBJECTIVE_BYTES>,
    pub(crate) constraints: Items<Text<MAX_TEXT_BYTES>, MAX_RULES>,
    pub(crate) steps: Items<PlanStep, MAX_ITEMS>,
    pub(crate) checks: Items<AcceptanceCheck, MAX_CHECKS>,
    pub(crate) risks: Items<Text<MAX_TEXT_BYTES>, MAX_RULES>,
    pub(crate) assumptions: Items<Text<MAX_TEXT_BYTES>, MAX_RULES>,
    pub(crate) non_goals: Items<Text<MAX_TEXT_BYTES>, MAX_RULES>,
    pub(crate) markdown: Text<MAX_MARKDOWN_BYTES>,
    pub(crate) digest: Digest,
}

impl PlanRevision {
    pub(crate) fn id(&self) -> PlanRevisionId {
        self.id
    }

    pub(crate) fn work(&self) -> WorkId {
        self.work
    }

    pub(crate) fn parent(&self) -> Option<PlanRevisionId> {
        self.parent
    }

    pub(crate) fn digest(&self) -> Digest {
        self.digest
    }

    pub(crate) fn markdown(&self) -> &str {
        self.markdown.as_str()
    }

    pub(crate) fn validate(&self) -> Result<(), WorkError> {
        if self.format != 1 {
            return Err(WorkError::PlanFormat);
        }
        let mut ids: Vec<_> = self.steps.iter().map(|step| step.id).collect();
        ids.sort_unstable();
        if ids.windows(2).any(|pair| pair[0] == pair[1]) {
            return Err(WorkError::ItemOrder);
        }
        if self.steps.iter().any(|step| {
            let dependencies: HashSet<_> = step.dependencies.iter().copied().collect();
            dependencies.len() != step.dependencies.len()
                || step.dependencies.iter().any(|dependency| {
                    *dependency == step.id || ids.binary_search(dependency).is_err()
                })
        }) {
            return Err(WorkError::Dependency);
        }
        let checks: HashSet<_> = self.checks.iter().map(|check| check.id).collect();
        if checks.len() != self.checks.len() {
            return Err(WorkError::PlanIdentity);
        }
        if self.digest != self.calculated_digest()? {
            return Err(WorkError::PlanDigest);
        }
        Ok(())
    }

    pub(crate) fn calculated_digest(&self) -> Result<Digest, WorkError> {
        let body = PlanBody {
            format: self.format,
            id: self.id,
            work: self.work,
            parent: self.parent,
            source: &self.source,
            objective: &self.objective,
            constraints: &self.constraints,
            steps: &self.steps,
            checks: &self.checks,
            risks: &self.risks,
            assumptions: &self.assumptions,
            non_goals: &self.non_goals,
            markdown: &self.markdown,
        };
        let bytes = to_vec(&body).map_err(|error| WorkError::Encoding(error.to_string()))?;
        Ok(Digest::hash(b"mage:plan:v1", &bytes))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct ReviewerHead {
    pub(crate) endpoint: Text<2_048>,
    pub(crate) model: Text<1_024>,
    pub(crate) request_format: u16,
    pub(crate) token_limit: u64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct VerificationPolicy {
    pub(crate) id: VerificationPolicyId,
    pub(crate) scope: WorkspaceScope,
    pub(crate) checks: Items<CheckId, MAX_CHECKS>,
    pub(crate) artifacts: Items<ArtifactRoot, MAX_ARTIFACTS>,
    pub(crate) coverage_rules: Items<CoverageRuleId, MAX_RULES>,
    pub(crate) reviewer: ReviewerHead,
    pub(crate) repair_limit: u16,
}

impl VerificationPolicy {
    pub(crate) fn id(&self) -> VerificationPolicyId {
        self.id
    }

    pub(crate) fn scope(&self) -> WorkspaceScopeId {
        self.scope.id()
    }

    pub(crate) fn same_capture(&self, other: &Self) -> bool {
        self.scope == other.scope && self.artifacts == other.artifacts
    }
}

#[derive(Serialize)]
struct CandidateBody {
    work: WorkId,
    execute_revision: WorkRevision,
    plan: PlanRevisionId,
    policy: VerificationPolicyId,
    scope: WorkspaceScopeId,
    base_digest: Digest,
    capture_digest: Digest,
    manifest_digest: Digest,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct Candidate {
    pub(crate) id: CandidateId,
    pub(crate) work: WorkId,
    pub(crate) execute_revision: WorkRevision,
    pub(crate) plan: PlanRevisionId,
    pub(crate) policy: VerificationPolicyId,
    pub(crate) scope: WorkspaceScopeId,
    pub(crate) base: ObservationId,
    pub(crate) capture: ObservationId,
    pub(crate) manifest_digest: Digest,
    pub(crate) digest: Digest,
}

impl Candidate {
    pub(crate) fn id(&self) -> CandidateId {
        self.id
    }

    pub(crate) fn calculated_digest(
        &self,
        base_digest: Digest,
        capture_digest: Digest,
    ) -> Result<Digest, WorkError> {
        let body = CandidateBody {
            work: self.work,
            execute_revision: self.execute_revision,
            plan: self.plan,
            policy: self.policy,
            scope: self.scope,
            base_digest,
            capture_digest,
            manifest_digest: self.manifest_digest,
        };
        let bytes = to_vec(&body).map_err(|error| WorkError::Encoding(error.to_string()))?;
        Ok(Digest::hash(b"mage:candidate:v1", &bytes))
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(tag = "projection", content = "owner", rename_all = "snake_case")]
pub(crate) enum WorkProjection {
    Context(ContextEntryId),
    Epoch(ContextEpochId),
    Pending(ToolBatchId),
    Operation(OperationId),
    Plan(PlanRevisionId),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub(crate) struct WorkState {
    pub(crate) id: WorkId,
    pub(crate) revision: WorkRevision,
    pub(crate) objective: Text<MAX_OBJECTIVE_BYTES>,
    pub(crate) status: WorkStatus,
    pub(crate) policy: WorkPolicy,
    pub(crate) selected_plan: Option<PlanRevisionId>,
    pub(crate) items: Items<WorkItem, MAX_ITEMS>,
    pub(crate) active_item: Option<WorkItemId>,
    pub(crate) constraints: Items<Text<MAX_TEXT_BYTES>, MAX_RULES>,
    pub(crate) acceptance_checks: Items<CheckId, MAX_CHECKS>,
    pub(crate) scope: Option<WorkspaceScopeId>,
    pub(crate) base: Option<ObservationId>,
    pub(crate) candidate: Option<Candidate>,
    pub(crate) verification: Option<VerificationPolicy>,
    pub(crate) evidence: Items<EvidenceRef, MAX_EVIDENCE>,
    pub(crate) budgets: StageBudgets,
    pub(crate) predecessor: Option<WorkId>,
    pub(crate) projection: Text<MAX_PROJECTION_BYTES>,
    pub(crate) projection_owner: WorkProjection,
}

impl WorkState {
    pub(crate) fn id(&self) -> WorkId {
        self.id
    }

    pub(crate) fn revision(&self) -> WorkRevision {
        self.revision
    }

    pub(crate) fn status(&self) -> &WorkStatus {
        &self.status
    }

    pub(crate) fn selected_plan(&self) -> Option<PlanRevisionId> {
        self.selected_plan
    }

    pub(crate) fn scope(&self) -> Option<WorkspaceScopeId> {
        self.scope
    }

    pub(crate) fn base(&self) -> Option<ObservationId> {
        self.base
    }

    pub(crate) fn candidate(&self) -> Option<&Candidate> {
        self.candidate.as_ref()
    }

    pub(crate) fn verification(&self) -> Option<&VerificationPolicy> {
        self.verification.as_ref()
    }

    pub(crate) fn projection_owner(&self) -> &WorkProjection {
        &self.projection_owner
    }

    pub(crate) fn validate(&self) -> Result<(), WorkError> {
        if self.revision.get() == 0 {
            return Err(WorkError::Revision);
        }
        let ids: Vec<_> = self.items.iter().map(|item| item.id).collect();
        let unique: HashSet<_> = ids.iter().copied().collect();
        if unique.len() != ids.len() {
            return Err(WorkError::ItemOrder);
        }
        if self
            .active_item
            .is_some_and(|active| !ids.contains(&active))
        {
            return Err(WorkError::ActiveItem);
        }
        if self.status.terminal() && self.active_item.is_some() {
            return Err(WorkError::TerminalActiveItem);
        }
        if matches!(self.status, WorkStatus::Completed { .. })
            && self.items.iter().any(|item| {
                !matches!(
                    item.disposition,
                    ItemDisposition::Completed | ItemDisposition::Retired
                )
            })
        {
            return Err(WorkError::CandidateBinding);
        }
        match self.status.stage() {
            Some(WorkStage::Plan) | None => {}
            Some(WorkStage::Execute | WorkStage::Verify) => {
                if self.selected_plan.is_none() || self.verification.is_none() {
                    return Err(WorkError::Selection);
                }
                if self.acceptance_checks.is_empty() {
                    return Err(WorkError::Selection);
                }
                if self.scope.is_none() || self.base.is_none() {
                    return Err(WorkError::Base);
                }
            }
        }
        if matches!(self.status.stage(), Some(WorkStage::Verify)) && self.candidate.is_none() {
            return Err(WorkError::Candidate);
        }
        if let WorkStatus::Completed { candidate, verdict } = self.status
            && (self.candidate.as_ref().map(Candidate::id) != Some(candidate)
                || !self.evidence.iter().any(|evidence| evidence.id == verdict))
        {
            return Err(WorkError::Candidate);
        }
        if let Some(candidate) = &self.candidate {
            let policy = self.verification.as_ref().map(VerificationPolicy::id);
            if candidate.work != self.id
                || Some(candidate.plan) != self.selected_plan
                || Some(candidate.policy) != policy
                || Some(candidate.scope) != self.scope
                || Some(candidate.base) != self.base
            {
                return Err(WorkError::CandidateBinding);
            }
        }
        if let (Some(scope), Some(policy)) = (self.scope, self.verification.as_ref()) {
            if policy.scope() != scope {
                return Err(WorkError::Base);
            }
            policy.scope.validate()?;
        }
        let evidence: HashSet<_> = self.evidence.iter().map(|evidence| evidence.id).collect();
        if evidence.len() != self.evidence.len()
            || self
                .items
                .iter()
                .flat_map(|item| item.evidence.iter())
                .any(|id| !evidence.contains(id))
        {
            return Err(WorkError::CandidateBinding);
        }
        for budget in [
            &self.budgets.plan,
            &self.budgets.execute,
            &self.budgets.verify,
        ] {
            if budget
                .token_limit
                .is_some_and(|limit| budget.token_reserve > limit)
                || budget
                    .live_millis_limit
                    .is_some_and(|limit| budget.live_millis_reserve > limit)
            {
                return Err(WorkError::StageBudget);
            }
        }
        Ok(())
    }
}

pub(crate) fn plan_path(
    platform: PathPlatform,
    session: SessionId,
    work: WorkId,
    revision: PlanRevisionId,
) -> Result<PathKey, WorkspaceError> {
    let component = |value: String| match platform {
        PathPlatform::Unix => PathComponent::unix(value.into_bytes()),
        PathPlatform::Windows => PathComponent::windows(value.encode_utf16().collect()),
    };
    PathKey::new(
        platform,
        vec![
            component(".mage".to_string())?,
            component("plans".to_string())?,
            component(session.path_component())?,
            component(work.path_component())?,
            component(format!("{}.md", revision.path_component()))?,
        ],
    )
}
