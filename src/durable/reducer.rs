use std::collections::{HashMap, HashSet};
use std::result::Result as StdResult;

use thiserror::Error;

use super::context::{ContextEntry, ContextEpoch, InputRoute, OperationClose, UserPayload};
use super::effect::{
    BudgetKind, EffectError, MeterSettlement, ProviderIntent, ProviderOutcome, ToolCall,
    ToolIntent, ToolResult,
};
use super::encoding::{Commit, EncodingError, FORMAT};
use super::fact::{CommandReceipt, CompletionCheck, FactData, FactKind, MaterializationOutcome};
use super::id::{
    CheckId, CommandId, ContextEntryId, ContextEpochId, Digest, EvidenceId, FactId, ForkId,
    InputId, MaterializationId, ObservationId, OperationId, PlanRevisionId, ProviderAttemptId,
    ScopedId, SessionId, ToolAttemptId, ToolBatchId, ToolCallId, ToolResultId, WorkId,
    WorkspaceScopeId,
};
use super::work::{PlanRevision, WorkError, WorkStage, WorkState};
use super::workspace::{WorkspaceError, WorkspaceObservation, WorkspaceScope};

#[path = "reducer/groups.rs"]
mod groups;
#[path = "reducer/history.rs"]
mod history;
#[path = "reducer/namespace.rs"]
mod namespace;
#[path = "reducer/unique.rs"]
mod unique;
#[path = "reducer/validate.rs"]
mod validate;

use history::History;

#[derive(Debug, Error)]
pub(crate) enum ReduceError {
    #[error("canonical JSON encoding failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Encoding(#[from] EncodingError),
    #[error(transparent)]
    Effect(#[from] EffectError),
    #[error(transparent)]
    Work(#[from] WorkError),
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),
    #[error("commit uses format {actual}; expected {expected}")]
    Format { actual: u16, expected: u16 },
    #[error("commit sequence is {actual}; expected {expected}")]
    Sequence { actual: u64, expected: u64 },
    #[error("commit parent does not match the accepted chain head")]
    Parent,
    #[error("identity in {field} does not belong to session {session}")]
    Namespace {
        field: &'static str,
        session: SessionId,
    },
    #[error("commit contains duplicate {kind} identity")]
    Duplicate { kind: &'static str },
    #[error("commit atomic group is invalid: {0}")]
    Group(&'static str),
    #[error("state transition is invalid: {0}")]
    State(&'static str),
    #[error("commit refers to unknown {kind}")]
    Unknown { kind: &'static str },
}

pub(crate) type Result<T> = StdResult<T, ReduceError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum InputState {
    Pending(InputRoute),
    Applied,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OpenOperation {
    pub(crate) id: OperationId,
    pub(crate) input: InputId,
    pub(crate) work: Option<WorkId>,
    pub(crate) stage: Option<WorkStage>,
    pub(crate) aborting: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ProviderState {
    Started(ProviderIntent),
    Finished {
        intent: ProviderIntent,
        outcome: ProviderOutcome,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ToolState {
    pub(crate) call: ToolCall,
    pub(crate) intent: Option<ToolIntent>,
    pub(crate) result: Option<ToolResult>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Materialization {
    pub(crate) plan: PlanRevisionId,
    pub(crate) digest: Digest,
    pub(crate) outcome: Option<MaterializationOutcome>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OpenFork {
    pub(crate) id: ForkId,
    pub(crate) child: SessionId,
    pub(crate) source_seq: u64,
    pub(crate) source_digest: Digest,
    pub(crate) observation: ObservationId,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct MeterState {
    pub(crate) settled: Vec<MeterSettlement>,
    pub(crate) limits: HashMap<BudgetKind, Option<u64>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SessionState {
    pub(crate) session: Option<SessionId>,
    pub(crate) seq: u64,
    pub(crate) head: Option<Digest>,
    pub(crate) facts: HashMap<FactId, FactKind>,
    receipts: History<CommandId, CommandReceipt>,
    pub(crate) default_scope: Option<WorkspaceScopeId>,
    pub(crate) scopes: HashMap<WorkspaceScopeId, WorkspaceScope>,
    pub(crate) plans: HashMap<PlanRevisionId, PlanRevision>,
    pub(crate) materializations: HashMap<MaterializationId, Materialization>,
    pub(crate) open_materialization: Option<MaterializationId>,
    pub(crate) works: HashMap<WorkId, WorkState>,
    pub(crate) evidence: HashSet<EvidenceId>,
    pub(crate) observations: HashMap<ObservationId, WorkspaceObservation>,
    pub(crate) inputs: HashMap<InputId, InputState>,
    pub(crate) input_payloads: HashMap<InputId, UserPayload>,
    pub(crate) operation: Option<OpenOperation>,
    closed_operations: History<OperationId, OperationClose>,
    pub(crate) providers: HashMap<ProviderAttemptId, ProviderState>,
    pub(crate) open_provider: Option<ProviderAttemptId>,
    pub(crate) completion_checks: HashMap<(OperationId, CheckId), bool>,
    pub(crate) tools: HashMap<ToolCallId, ToolState>,
    pub(crate) batches: HashMap<ToolBatchId, Vec<ToolCallId>>,
    pub(crate) tool_attempts: HashMap<ToolAttemptId, ToolCallId>,
    pub(crate) tool_results: HashSet<ToolResultId>,
    pub(crate) epochs: HashMap<ContextEpochId, ContextEpoch>,
    pub(crate) epoch_entries: HashMap<ContextEpochId, Vec<ContextEntryId>>,
    pub(crate) current_epoch: Option<ContextEpochId>,
    pub(crate) entries: HashMap<ContextEntryId, ContextEntry>,
    pub(crate) open_fork: Option<OpenFork>,
    pub(crate) forks: HashMap<ForkId, (Digest, ObservationId)>,
    pub(crate) meters: MeterState,
    next_local: u128,
}

impl SessionState {
    pub(crate) fn session(&self) -> Option<SessionId> {
        self.session
    }

    pub(crate) fn seq(&self) -> u64 {
        self.seq
    }

    pub(crate) fn next_local(&self) -> u128 {
        self.next_local
    }

    pub(crate) fn head(&self) -> Option<Digest> {
        self.head
    }

    pub(crate) fn receipt(&self, id: CommandId) -> Option<&CommandReceipt> {
        self.receipts.get(id)
    }

    pub(crate) fn maybe_receipt(&self, id: CommandId) -> bool {
        self.receipts.might_contain(id)
    }

    pub(crate) fn closed_operation(&self, id: OperationId) -> Option<&OperationClose> {
        self.closed_operations.get(id)
    }

    pub(crate) fn maybe_closed(&self, id: OperationId) -> bool {
        self.closed_operations.might_contain(id)
    }

    #[cfg(test)]
    pub(crate) fn hot_receipts(&self) -> usize {
        self.receipts.len()
    }

    #[cfg(test)]
    pub(crate) fn hot_operations(&self) -> usize {
        self.closed_operations.len()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedCommit {
    commit: Commit,
}

impl PreparedCommit {
    pub(crate) fn commit(&self) -> &Commit {
        &self.commit
    }
}

pub(crate) fn prepare(state: &SessionState, commit: Commit) -> Result<PreparedCommit> {
    if commit.format != FORMAT {
        return Err(ReduceError::Format {
            actual: commit.format,
            expected: FORMAT,
        });
    }
    let expected = state.seq.saturating_add(1);
    if commit.seq != expected {
        return Err(ReduceError::Sequence {
            actual: commit.seq,
            expected,
        });
    }
    if commit.previous != state.head {
        return Err(ReduceError::Parent);
    }
    commit.validate_digest()?;

    let session = namespace::session(state, &commit)?;
    namespace::validate(state, &commit, session)?;
    unique::validate(&commit)?;
    groups::validate(state, &commit)?;
    validate::transition(state, &commit)?;
    Ok(PreparedCommit { commit })
}

pub(crate) fn apply(state: &mut SessionState, prepared: PreparedCommit) {
    let commit = prepared.commit;
    let seq = commit.seq;
    let digest = commit.digest;
    note_id(state, commit.id);
    if let Some(receipt) = &commit.command {
        note_id(state, receipt.id);
        if let Some(result) = receipt.result {
            note_id(state, result);
        }
    }
    if let Some(receipt) = commit.command {
        state.receipts.insert(receipt.id, receipt);
    }
    for fact in commit.facts {
        note_id(state, fact.id);
        state.facts.insert(fact.id, fact.kind());
        apply_fact(state, fact.data);
    }
    state.seq = seq;
    state.head = Some(digest);
}

fn note_id<K>(state: &mut SessionState, id: ScopedId<K>) {
    let local = id.local_u128();
    if local >= state.next_local {
        state.next_local = local.saturating_add(1);
    }
}

fn apply_fact(state: &mut SessionState, fact: FactData) {
    match fact {
        FactData::SessionCreated { session, scope } => {
            state.session = Some(session);
            state.default_scope = Some(scope.id());
            state.scopes.insert(scope.id(), scope);
        }
        FactData::BudgetChanged { kind, limit } => {
            state.meters.limits.insert(kind, limit);
        }
        FactData::PlanSet { plan } => {
            state.plans.insert(plan.id(), plan);
        }
        FactData::PlanMaterializationPlanned {
            attempt,
            plan,
            digest,
            ..
        } => {
            state.materializations.insert(
                attempt,
                Materialization {
                    plan,
                    digest,
                    outcome: None,
                },
            );
            state.open_materialization = Some(attempt);
        }
        FactData::PlanMaterializationFinished {
            attempt, outcome, ..
        } => {
            if let Some(materialization) = state.materializations.get_mut(&attempt) {
                materialization.outcome = Some(outcome);
            }
            state.open_materialization = None;
        }
        FactData::ForkPlanned {
            intent,
            child,
            source_seq,
            source_digest,
            observation,
        } => {
            state.open_fork = Some(OpenFork {
                id: intent,
                child,
                source_seq,
                source_digest,
                observation,
            });
        }
        FactData::ForkFinished {
            intent,
            sealed_journal,
            observation,
        } => {
            state.forks.insert(intent, (sealed_journal, observation));
            state.open_fork = None;
        }
        FactData::ForkedFrom { .. }
        | FactData::ForkSnapshotSealed { .. }
        | FactData::CommandRejected { .. } => {}
        FactData::InputAccepted {
            input,
            route,
            payload,
            ..
        } => {
            state.input_payloads.insert(input, payload);
            state.inputs.insert(input, InputState::Pending(route));
        }
        FactData::InputApplied { input, .. } => {
            state.inputs.insert(input, InputState::Applied);
        }
        FactData::WorkSet { work } => {
            let work = *work;
            state
                .evidence
                .extend(work.evidence.iter().map(|evidence| evidence.id));
            state.works.insert(work.id(), work);
        }
        FactData::WorkspaceObserved { observation } => {
            state.observations.insert(observation.id(), observation);
        }
        FactData::OperationOpened {
            operation,
            input,
            work,
            stage,
        } => {
            state.operation = Some(OpenOperation {
                id: operation,
                input,
                work,
                stage,
                aborting: false,
            });
        }
        FactData::AbortRequested { .. } => {
            if let Some(operation) = &mut state.operation {
                operation.aborting = true;
            }
        }
        FactData::OperationClosed { close, .. } => {
            state.closed_operations.insert(close.id, close);
            state.operation = None;
        }
        FactData::ProviderStarted { intent } => {
            let attempt = intent.attempt;
            state
                .providers
                .insert(attempt, ProviderState::Started(intent));
            state.open_provider = Some(attempt);
        }
        FactData::ProviderFinished {
            attempt, outcome, ..
        } => {
            if let Some(ProviderState::Started(intent)) = state.providers.remove(&attempt) {
                state
                    .providers
                    .insert(attempt, ProviderState::Finished { intent, outcome });
            }
            state.open_provider = None;
        }
        FactData::CompletionCheckRecorded {
            operation,
            check: CompletionCheck::Declared(check),
            passed,
            ..
        } => {
            state.completion_checks.insert((operation, check), passed);
        }
        FactData::CompletionCheckRecorded { .. } => {}
        FactData::ToolCallPlanned { call } => {
            state.batches.entry(call.batch).or_default().push(call.id);
            let id = call.id;
            state.tools.insert(
                id,
                ToolState {
                    call,
                    intent: None,
                    result: None,
                },
            );
        }
        FactData::ToolStarted { intent } => {
            let attempt = intent.attempt;
            let call = intent.call;
            state.tool_attempts.insert(attempt, call);
            if let Some(tool) = state.tools.get_mut(&call) {
                tool.intent = Some(intent);
            }
        }
        FactData::ToolFinished { result } => {
            let id = result.id;
            let call = result.call;
            state.tool_results.insert(id);
            if let Some(tool) = state.tools.get_mut(&call) {
                tool.result = Some(result);
            }
        }
        FactData::ContextEpochOpened { epoch } => {
            let id = epoch.id;
            let entries = epoch.base.as_slice().to_vec();
            state.epochs.insert(id, epoch);
            state.epoch_entries.insert(id, entries);
            state.current_epoch = Some(id);
        }
        FactData::ContextEntryAdded { entry } => {
            let id = entry.id;
            let epoch = entry.epoch;
            state.entries.insert(id, entry);
            state.epoch_entries.entry(epoch).or_default().push(id);
        }
        FactData::ConversationCleared { .. } => {}
        FactData::MeterSettled { settlement, .. } => {
            state.meters.settled.push(settlement);
        }
    }
}

pub(crate) fn fact_kinds(commit: &Commit) -> HashSet<FactKind> {
    commit.facts.iter().map(|fact| fact.kind()).collect()
}
