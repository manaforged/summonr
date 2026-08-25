use std::collections::HashSet;

use super::{ReduceError, Result, SessionState};
use crate::durable::context::{ContextSource, InputOrigin, InputRoute, OperationOutcome};
use crate::durable::effect::{
    DetachedClose, ReconciliationContract, ToolOutcome, ToolResult, WorkspaceFootprint,
};
use crate::durable::encoding::Commit;
use crate::durable::fact::{
    CommandStatus, CompletionCheck, FactData, FactKind, ForkOutcome, MeterOwner,
    OperationStageOutcome,
};
use crate::durable::id::{ToolBatchId, ToolCallId};
use crate::durable::work::{WorkPolicy, WorkProjection, WorkStage, WorkState};

pub(super) fn validate(state: &SessionState, commit: &Commit) -> Result<()> {
    let facts = Facts { commit };
    if state.seq == 0 {
        root(&facts)?;
    } else if facts.has(FactKind::SessionCreated) || facts.has(FactKind::ForkedFrom) {
        return Err(ReduceError::Group(
            "publication root fact appears after sequence 1",
        ));
    }

    command(&facts)?;
    plans(state, &facts)?;
    input(&facts)?;
    work(state, &facts)?;
    operation(&facts)?;
    provider(&facts)?;
    completion_checks(state, &facts)?;
    tools(state, &facts)?;
    retry(&facts)?;
    fork(&facts)?;
    context(&facts)?;
    meters(&facts)?;
    Ok(())
}

fn root(facts: &Facts<'_>) -> Result<()> {
    if facts.count(FactKind::SessionCreated) != 1 || facts.count(FactKind::ContextEpochOpened) != 1
    {
        return Err(ReduceError::Group(
            "session creation requires one SessionCreated and one ContextEpochOpened",
        ));
    }
    let child = facts.has(FactKind::ForkedFrom);
    let allowed = if child {
        [
            FactKind::SessionCreated,
            FactKind::ForkedFrom,
            FactKind::ContextEpochOpened,
        ]
        .as_slice()
    } else {
        [FactKind::SessionCreated, FactKind::ContextEpochOpened].as_slice()
    };
    if facts
        .commit
        .facts
        .iter()
        .any(|fact| !allowed.contains(&fact.kind()))
    {
        return Err(ReduceError::Group(
            "session root contains an unrelated fact",
        ));
    }
    Ok(())
}

fn command(facts: &Facts<'_>) -> Result<()> {
    let rejected = facts.count(FactKind::CommandRejected);
    match (&facts.commit.command, rejected) {
        (Some(receipt), 1) if receipt.status == CommandStatus::Rejected => {}
        (Some(receipt), 0) if receipt.status == CommandStatus::Accepted => {}
        (None, 0) => {}
        _ => {
            return Err(ReduceError::Group(
                "CommandRejected and a rejected command receipt must appear together",
            ));
        }
    }
    if let Some(receipt) = &facts.commit.command
        && let Some(result) = receipt.result
        && !facts.commit.facts.iter().any(|fact| fact.id == result)
    {
        return Err(ReduceError::Group(
            "command receipt result must name a fact in the same commit",
        ));
    }
    Ok(())
}

fn plans(state: &SessionState, facts: &Facts<'_>) -> Result<()> {
    for plan in facts.data(FactKind::PlanSet) {
        let FactData::PlanSet { plan } = plan else {
            continue;
        };
        let planned = facts.commit.facts.iter().any(|fact| {
            matches!(
                &fact.data,
                FactData::PlanMaterializationPlanned {
                    plan: revision,
                    digest,
                    ..
                } if *revision == plan.id() && *digest == plan.digest()
            )
        });
        let work = facts.commit.facts.iter().any(
            |fact| matches!(&fact.data, FactData::WorkSet { work } if work.id() == plan.work()),
        );
        if !planned || !work {
            return Err(ReduceError::Group(
                "PlanSet requires its materialization intent and complete Work revision",
            ));
        }
    }
    for planned in facts.data(FactKind::PlanMaterializationPlanned) {
        let FactData::PlanMaterializationPlanned { plan, .. } = planned else {
            continue;
        };
        let new_plan = facts.commit.facts.iter().any(
            |fact| matches!(&fact.data, FactData::PlanSet { plan: value } if value.id() == *plan),
        );
        let retry = facts.commit.command.is_some() && state.plans.contains_key(plan);
        if !new_plan && !retry {
            return Err(ReduceError::Group(
                "materialization intent requires a new Plan or typed retry command",
            ));
        }
    }
    for finished in facts.data(FactKind::PlanMaterializationFinished) {
        let FactData::PlanMaterializationFinished { plan, .. } = finished else {
            continue;
        };
        if !facts.commit.facts.iter().any(
            |fact| matches!(&fact.data, FactData::WorkSet { work } if work.selected_plan() == Some(*plan) || work.status().stage() == Some(WorkStage::Plan)),
        ) {
            return Err(ReduceError::Group(
                "materialization terminal requires its complete Work revision",
            ));
        }
    }
    Ok(())
}

fn input(facts: &Facts<'_>) -> Result<()> {
    for applied in facts.data(FactKind::InputApplied) {
        let FactData::InputApplied { operation, .. } = applied else {
            continue;
        };
        let legal = match operation {
            Some(operation) => {
                let opened = facts.commit.facts.iter().any(|fact| {
                    matches!(&fact.data, FactData::OperationOpened { operation: id, .. } if id == operation)
                });
                let steered = facts.has(FactKind::ContextEntryAdded)
                    && facts.commit.facts.iter().any(|fact| {
                        matches!(
                            &fact.data,
                            FactData::InputAccepted {
                                route: InputRoute::Steer,
                                ..
                            }
                        )
                    });
                opened || steered
            }
            None => facts.has(FactKind::DirectiveChanged) && facts.has(FactKind::ContextEntryAdded),
        };
        if !legal {
            return Err(ReduceError::Group(
                "InputApplied requires operation start or steering context update",
            ));
        }
    }
    Ok(())
}

fn work(state: &SessionState, facts: &Facts<'_>) -> Result<()> {
    for data in facts.data(FactKind::WorkSet) {
        let FactData::WorkSet { work } = data else {
            continue;
        };
        let legal = match work.projection_owner() {
            WorkProjection::Context(id) => facts.commit.facts.iter().any(|fact| {
                matches!(&fact.data, FactData::ContextEntryAdded { entry } if entry.id == *id)
            }),
            WorkProjection::Epoch(id) => facts.commit.facts.iter().any(|fact| {
                matches!(&fact.data, FactData::ContextEpochOpened { epoch } if epoch.id == *id)
            }),
            WorkProjection::Pending(batch) => facts.commit.facts.iter().any(|fact| {
                matches!(&fact.data, FactData::ToolFinished { result } if facts.batch_of(state, result.call) == Some(*batch))
            }),
            WorkProjection::CompletionCheck(check) => {
                state.facts.get(check) == Some(&FactKind::CompletionCheckRecorded)
                    || facts.commit.facts.iter().any(|fact| {
                        fact.id == *check
                            && matches!(fact.data, FactData::CompletionCheckRecorded { .. })
                    })
            }
            WorkProjection::Operation(id) => facts.commit.facts.iter().any(|fact| {
                matches!(&fact.data, FactData::OperationClosed { close, .. } if close.id == *id)
            }),
            WorkProjection::Plan(id) => facts.commit.facts.iter().any(|fact| {
                matches!(&fact.data, FactData::PlanSet { plan } if plan.id() == *id)
            }),
        };
        if !legal {
            return Err(ReduceError::Group(
                "WorkSet requires its exact context, epoch, batch, operation, or Plan owner",
            ));
        }
        if state.works.get(&work.id()).is_some_and(|current| {
            current.status().stage() == Some(WorkStage::Verify)
                && work.status().stage() == Some(WorkStage::Execute)
        }) {
            verify_return(state, facts, work)?;
        }
    }
    Ok(())
}

fn verify_return(state: &SessionState, facts: &Facts<'_>, work: &WorkState) -> Result<()> {
    if let WorkProjection::Operation(operation) = work.projection_owner() {
        let close = facts.commit.facts.iter().any(|fact| {
            matches!(
                &fact.data,
                FactData::OperationClosed {
                    close,
                    stage_outcome: OperationStageOutcome::Returned,
                } if close.id == *operation
                    && close.outcome == OperationOutcome::Completed
                    && close.candidate == work.candidate().map(|candidate| candidate.id())
            )
        });
        let epoch = facts.commit.facts.iter().any(|fact| {
            matches!(&fact.data, FactData::ContextEpochOpened { epoch } if epoch.stage == Some(WorkStage::Execute))
        });
        if work.policy() == WorkPolicy::Single || work.candidate().is_none() || !close || !epoch {
            return Err(ReduceError::Group(
                "Verify continuation requires an external Work candidate, close, and Execute epoch",
            ));
        }
        return Ok(());
    }

    let terminal_facts: HashSet<_> = match work.projection_owner() {
        WorkProjection::Pending(batch) => facts
            .commit
            .facts
            .iter()
            .filter_map(|fact| {
                matches!(&fact.data, FactData::ToolFinished { result } if facts.batch_of(state, result.call) == Some(*batch))
                    .then_some(fact.id)
            })
            .collect(),
        WorkProjection::CompletionCheck(check)
            if state.facts.get(check) == Some(&FactKind::CompletionCheckRecorded)
                || facts.commit.facts.iter().any(|fact| fact.id == *check) =>
        {
            HashSet::from([*check])
        }
        _ => {
            return Err(ReduceError::Group(
                "Verify failure must be owned by its terminal check",
            ));
        }
    };
    let current = state
        .works
        .get(&work.id())
        .ok_or(ReduceError::Group("Verify return has no current Work"))?;
    let finding = work.evidence.iter().any(|evidence| {
        terminal_facts.contains(&evidence.source)
            && !current.evidence.iter().any(|known| known.id == evidence.id)
    });
    let epoch = facts.commit.facts.iter().any(|fact| {
        matches!(&fact.data, FactData::ContextEpochOpened { epoch } if epoch.stage == Some(WorkStage::Execute))
    });
    let continuation = facts.commit.facts.iter().any(|fact| {
        matches!(
            &fact.data,
            FactData::InputAccepted {
                origin: InputOrigin::WorkPolicy,
                route: InputRoute::Queue,
                ..
            }
        )
    });
    if work.candidate().is_some()
        || !finding
        || !epoch
        || !continuation
        || !facts.has(FactKind::OperationClosed)
    {
        return Err(ReduceError::Group(
            "Verify failure requires finding, cleared candidate, close, Execute epoch, and continuation",
        ));
    }
    Ok(())
}

fn operation(facts: &Facts<'_>) -> Result<()> {
    if facts.count(FactKind::OperationOpened) > 1 || facts.count(FactKind::OperationClosed) > 1 {
        return Err(ReduceError::Group(
            "a commit can open or close at most one operation",
        ));
    }
    for opened in facts.data(FactKind::OperationOpened) {
        let FactData::OperationOpened {
            operation, input, ..
        } = opened
        else {
            continue;
        };
        let accepted = facts.commit.facts.iter().any(
            |fact| matches!(&fact.data, FactData::InputAccepted { input: id, .. } if id == input),
        );
        let applied = facts.commit.facts.iter().any(|fact| {
            matches!(&fact.data, FactData::InputApplied { input: id, operation: Some(owner) } if id == input && owner == operation)
        });
        let projected = facts.commit.facts.iter().any(|fact| {
            matches!(
                &fact.data,
                FactData::ContextEntryAdded { entry }
                    if matches!(
                        entry.source,
                        ContextSource::Input(id) | ContextSource::WorkPolicy(id) if id == *input
                    )
            )
        });
        if !accepted || !applied || !projected {
            return Err(ReduceError::Group(
                "OperationOpened requires accepted and applied input plus context",
            ));
        }
    }
    Ok(())
}

fn provider(facts: &Facts<'_>) -> Result<()> {
    for finished in facts.data(FactKind::ProviderFinished) {
        let FactData::ProviderFinished {
            attempt, result, ..
        } = finished
        else {
            continue;
        };
        if !facts.has_meter(&MeterOwner::Provider(*attempt)) {
            return Err(ReduceError::Group(
                "ProviderFinished requires its MeterSettled fact",
            ));
        }
        let calls = facts
            .data(FactKind::ToolCallPlanned)
            .filter(|data| matches!(data, FactData::ToolCallPlanned { call } if call.provider_result == *result))
            .count();
        if calls > 0 && !facts.has(FactKind::ContextEntryAdded) {
            return Err(ReduceError::Group(
                "completed agent tool batch requires assistant context",
            ));
        }
    }
    for call in facts.data(FactKind::ToolCallPlanned) {
        let FactData::ToolCallPlanned { call } = call else {
            continue;
        };
        if !facts.commit.facts.iter().any(|fact| {
            matches!(&fact.data, FactData::ProviderFinished { result, .. } if *result == call.provider_result)
        }) {
            return Err(ReduceError::Group(
                "ToolCallPlanned requires its completed provider result",
            ));
        }
    }
    Ok(())
}

fn completion_checks(state: &SessionState, facts: &Facts<'_>) -> Result<()> {
    let mut seen = HashSet::new();
    for recorded in facts.data(FactKind::CompletionCheckRecorded) {
        let FactData::CompletionCheckRecorded {
            operation,
            check,
            command,
            passed,
            ..
        } = recorded
        else {
            continue;
        };
        let Some(open) = state
            .operation
            .as_ref()
            .filter(|open| open.id == *operation && open.stage == Some(WorkStage::Verify))
        else {
            return Err(ReduceError::Group(
                "completion check requires its open Verify operation",
            ));
        };
        if state.open_provider.is_some() && !facts.has(FactKind::ProviderFinished) {
            return Err(ReduceError::Group(
                "completion check requires a finished provider round",
            ));
        }
        match check {
            CompletionCheck::Declared(check) => {
                if !seen.insert(*check)
                    || state.completion_checks.contains_key(&(*operation, *check))
                {
                    return Err(ReduceError::Group(
                        "completion check can be recorded once per Verify operation",
                    ));
                }
                let work =
                    open.work
                        .and_then(|work| state.works.get(&work))
                        .ok_or(ReduceError::Group(
                            "declared completion check requires Work",
                        ))?;
                let plan = work
                    .selected_plan()
                    .and_then(|plan| state.plans.get(&plan))
                    .ok_or(ReduceError::Group(
                        "declared completion check requires a selected Plan",
                    ))?;
                let declared = plan
                    .checks
                    .iter()
                    .find(|candidate| candidate.id == *check)
                    .ok_or(ReduceError::Group(
                        "completion check is not declared by the selected Plan",
                    ))?;
                let active = match work.policy() {
                    WorkPolicy::Steps => work
                        .active_item
                        .and_then(|item| plan.steps.iter().find(|step| step.id == item))
                        .is_some_and(|step| step.checks.iter().any(|id| id == check)),
                    WorkPolicy::Single | WorkPolicy::Goal { .. } => true,
                };
                if !active || declared.command.as_str() != command.as_str() {
                    return Err(ReduceError::Group(
                        "completion check does not match the active Work item",
                    ));
                }
            }
            CompletionCheck::CandidateStable | CompletionCheck::Interrupted if *passed => {
                return Err(ReduceError::Group(
                    "controller completion checks cannot pass Work",
                ));
            }
            CompletionCheck::CandidateStable | CompletionCheck::Interrupted => {}
        }
    }
    Ok(())
}

fn tools(state: &SessionState, facts: &Facts<'_>) -> Result<()> {
    for started in facts.data(FactKind::ToolStarted) {
        let FactData::ToolStarted { intent } = started else {
            continue;
        };
        if matches!(intent.footprint, WorkspaceFootprint::Scope(_))
            && !facts.has(FactKind::WorkspaceObserved)
        {
            return Err(ReduceError::Group(
                "scoped ToolStarted requires its before observation",
            ));
        }
    }
    for finished in facts.data(FactKind::ToolFinished) {
        let FactData::ToolFinished { result } = finished else {
            continue;
        };
        let started = result
            .attempt
            .and_then(|attempt| state.tool_attempts.get(&attempt))
            .is_some();
        if result.outcome.needs_start() {
            if !started {
                return Err(ReduceError::Group(
                    "started ToolFinished outcome requires prior ToolStarted",
                ));
            }
            let attempt = result.attempt.ok_or(ReduceError::Group(
                "started ToolFinished outcome is missing its attempt",
            ))?;
            if !facts.has_meter(&MeterOwner::Tool(attempt)) {
                return Err(ReduceError::Group(
                    "started ToolFinished requires MeterSettled",
                ));
            }
            let scoped = state
                .tools
                .get(&result.call)
                .and_then(|tool| tool.intent.as_ref())
                .is_some_and(|intent| matches!(intent.footprint, WorkspaceFootprint::Scope(_)));
            if scoped && !facts.has(FactKind::WorkspaceObserved) {
                return Err(ReduceError::Group(
                    "scoped ToolFinished requires its after observation",
                ));
            }
        } else if result.attempt.is_some() || facts.metered(result) {
            return Err(ReduceError::Group(
                "pre-start ToolFinished must not name an attempt or meter settlement",
            ));
        }
        let process_uncertainty = result.outcome.uncertain()
            && state
                .tools
                .get(&result.call)
                .and_then(|tool| tool.intent.as_ref())
                .is_some_and(|intent| {
                    matches!(
                        intent.reconciliation,
                        ReconciliationContract::Process { .. }
                    )
                });
        if (matches!(&result.outcome, ToolOutcome::Completed { job: Some(_), .. })
            || process_uncertainty)
            && !facts.has(FactKind::DetachedJobOpened)
        {
            return Err(ReduceError::Group(
                "background or uncertain launch requires DetachedJobOpened",
            ));
        }
    }
    for observed in facts.data(FactKind::EffectObserved) {
        let FactData::EffectObserved {
            observation,
            observing_result,
        } = observed
        else {
            continue;
        };
        let terminal = facts.commit.facts.iter().any(|fact| {
            matches!(&fact.data, FactData::ToolFinished { result } if result.id == *observing_result)
        });
        if !terminal || (observation.workspace.is_some() && !facts.has(FactKind::WorkspaceObserved))
        {
            return Err(ReduceError::Group(
                "EffectObserved requires its observing tool terminal and workspace evidence",
            ));
        }
    }
    for closed in facts.data(FactKind::DetachedJobClosed) {
        let FactData::DetachedJobClosed { outcome, .. } = closed else {
            continue;
        };
        if *outcome == DetachedClose::AbandonedUnknown && facts.commit.command.is_none() {
            return Err(ReduceError::Group(
                "abandoning a detached job requires a command receipt",
            ));
        }
    }
    Ok(())
}

fn retry(facts: &Facts<'_>) -> Result<()> {
    for finished in facts.data(FactKind::RetryFinished) {
        let FactData::RetryFinished { retry, .. } = finished else {
            continue;
        };
        if !facts.has_meter(&MeterOwner::Retry(*retry))
            || (!facts.has(FactKind::ProviderStarted) && !facts.has(FactKind::OperationClosed))
        {
            return Err(ReduceError::Group(
                "RetryFinished requires settlement and next provider or operation close",
            ));
        }
    }
    Ok(())
}

fn fork(facts: &Facts<'_>) -> Result<()> {
    if facts.has(FactKind::ForkPlanned)
        && (facts.commit.command.is_none() || !facts.has(FactKind::WorkspaceObserved))
    {
        return Err(ReduceError::Group(
            "ForkPlanned requires command receipt and source observation",
        ));
    }
    if facts.has(FactKind::ForkCancelRequested) && facts.commit.command.is_none() {
        return Err(ReduceError::Group(
            "ForkCancelRequested requires command receipt",
        ));
    }
    for finished in facts.data(FactKind::ForkFinished) {
        if matches!(
            finished,
            FactData::ForkFinished {
                outcome: ForkOutcome::Published { .. },
                ..
            }
        ) && !facts.has(FactKind::WorkspaceObserved)
        {
            return Err(ReduceError::Group(
                "published ForkFinished requires parent observation",
            ));
        }
    }
    Ok(())
}

fn context(facts: &Facts<'_>) -> Result<()> {
    if facts.has(FactKind::ConversationCleared) && facts.count(FactKind::ContextEpochOpened) != 1 {
        return Err(ReduceError::Group(
            "ConversationCleared requires one replacement epoch",
        ));
    }
    if facts.has(FactKind::DirectiveChanged)
        && !facts.has(FactKind::ContextEntryAdded)
        && !facts.has(FactKind::ContextEpochOpened)
    {
        return Err(ReduceError::Group(
            "DirectiveChanged requires a context delta or replacement epoch",
        ));
    }
    Ok(())
}

fn meters(facts: &Facts<'_>) -> Result<()> {
    for meter in facts.data(FactKind::MeterSettled) {
        let FactData::MeterSettled { owner, .. } = meter else {
            continue;
        };
        let terminal = match owner {
            MeterOwner::Provider(attempt) => facts.commit.facts.iter().any(|fact| {
                matches!(&fact.data, FactData::ProviderFinished { attempt: id, .. } if id == attempt)
            }),
            MeterOwner::Tool(attempt) => facts.commit.facts.iter().any(|fact| {
                matches!(&fact.data, FactData::ToolFinished { result } if result.attempt == Some(*attempt))
            }),
            MeterOwner::Retry(retry) => facts.commit.facts.iter().any(|fact| {
                matches!(&fact.data, FactData::RetryFinished { retry: id, .. } if id == retry)
            }),
        };
        if !terminal {
            return Err(ReduceError::Group(
                "MeterSettled requires its terminal effect in the same commit",
            ));
        }
    }
    Ok(())
}

struct Facts<'a> {
    commit: &'a Commit,
}

impl<'a> Facts<'a> {
    fn count(&self, kind: FactKind) -> usize {
        self.commit
            .facts
            .iter()
            .filter(|fact| fact.kind() == kind)
            .count()
    }

    fn has(&self, kind: FactKind) -> bool {
        self.count(kind) > 0
    }

    fn data(&self, kind: FactKind) -> impl Iterator<Item = &'a FactData> {
        self.commit
            .facts
            .iter()
            .filter(move |fact| fact.kind() == kind)
            .map(|fact| &fact.data)
    }

    fn has_meter(&self, owner: &MeterOwner) -> bool {
        self.commit.facts.iter().any(|fact| {
            matches!(&fact.data, FactData::MeterSettled { owner: value, .. } if value == owner)
        })
    }

    fn metered(&self, result: &ToolResult) -> bool {
        result
            .attempt
            .is_some_and(|attempt| self.has_meter(&MeterOwner::Tool(attempt)))
    }

    fn batch_of(&self, state: &SessionState, call: ToolCallId) -> Option<ToolBatchId> {
        state
            .tools
            .get(&call)
            .map(|tool| tool.call.batch)
            .or_else(|| {
                self.commit.facts.iter().find_map(|fact| match &fact.data {
                    FactData::ToolCallPlanned { call: value } if value.id == call => {
                        Some(value.batch)
                    }
                    _ => None,
                })
            })
    }
}
