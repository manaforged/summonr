#[path = "validate/effect.rs"]
mod effect;
#[path = "validate/work.rs"]
mod work;

use std::collections::HashSet;

use super::{InputState, ProviderState, ReduceError, Result, SessionState, fact_kinds};
use crate::durable::context::{
    ContextEntry, ContextEpoch, ContextSource, Directive, DirectiveState, ENTRY_LIMIT, InputRoute,
};
use crate::durable::effect::{DetachedClose, ProviderOutcome, ReconciliationResolution};
use crate::durable::encoding::Commit;
use crate::durable::fact::{FactData, FactKind, ForkOutcome, MaterializationOutcome};
use crate::durable::id::{ContextEntryId, ContextEpochId, ObservationId, WorkspaceScopeId};
use crate::durable::workspace::WorkspaceObservation;
use effect::{
    effect_observed, fork_blocked, meter_owner, operation_close, operation_open, provider_finish,
    provider_start, tool_batch, tool_finish, tool_plan, tool_start,
};
use work::{find_plan, find_work, plan_path, validate_plan, validate_work};

pub(super) fn transition(state: &SessionState, commit: &Commit) -> Result<()> {
    if commit.facts.is_empty() {
        return Err(ReduceError::State("commit has no facts"));
    }
    blockers(state, commit)?;
    for fact in &commit.facts {
        validate_fact(state, commit, &fact.data)?;
    }
    tool_batch(state, commit)?;
    Ok(())
}

fn blockers(state: &SessionState, commit: &Commit) -> Result<()> {
    let kinds = fact_kinds(commit);
    if state.open_fork.is_some() {
        let permitted = [
            FactKind::ForkCancelRequested,
            FactKind::ForkFinished,
            FactKind::WorkspaceObserved,
            FactKind::CommandRejected,
        ];
        if kinds.iter().any(|kind| !permitted.contains(kind)) {
            return Err(ReduceError::State("open fork blocks parent mutation"));
        }
    }
    if state.open_materialization.is_some() {
        let finishes = kinds.contains(&FactKind::PlanMaterializationFinished);
        if kinds.contains(&FactKind::OperationOpened)
            || kinds.contains(&FactKind::ProviderStarted)
            || kinds.contains(&FactKind::ForkPlanned)
            || kinds.contains(&FactKind::PlanMaterializationPlanned)
            || (kinds.contains(&FactKind::ContextEpochOpened) && !finishes)
        {
            return Err(ReduceError::State(
                "open Plan materialization blocks operation, provider, rebase, and fork",
            ));
        }
    }
    if let Some(operation) = &state.operation {
        if kinds.contains(&FactKind::OperationOpened)
            || kinds.contains(&FactKind::ForkPlanned)
            || kinds.contains(&FactKind::ConversationCleared)
        {
            return Err(ReduceError::State(
                "open operation blocks another operation, fork, and clear",
            ));
        }
        if operation.aborting
            && (kinds.contains(&FactKind::ProviderStarted)
                || kinds.contains(&FactKind::ToolStarted)
                || kinds.contains(&FactKind::RetryScheduled))
        {
            return Err(ReduceError::State(
                "aborting operation cannot start a new external effect",
            ));
        }
    }
    Ok(())
}

fn validate_fact(state: &SessionState, commit: &Commit, fact: &FactData) -> Result<()> {
    match fact {
        FactData::SessionCreated { scope, .. } => {
            if state.session.is_some() {
                return Err(ReduceError::State("session already exists"));
            }
            scope.validate()?;
        }
        FactData::BudgetChanged { .. } => {
            if commit.command.is_none() {
                return Err(ReduceError::State(
                    "budget change requires a command receipt",
                ));
            }
        }
        FactData::PlanSet { plan } => validate_plan(state, commit, plan)?,
        FactData::PlanMaterializationPlanned {
            attempt,
            plan,
            digest,
            footprint,
        } => {
            if state.materializations.contains_key(attempt) || state.open_materialization.is_some()
            {
                return Err(ReduceError::State(
                    "Plan materialization is already known or open",
                ));
            }
            let revision = find_plan(state, commit, *plan).ok_or_else(|| unknown("Plan"))?;
            footprint.validate(&attempt.path_component())?;
            if revision.digest() != *digest || footprint.destination != plan_path(state, revision)?
            {
                return Err(ReduceError::State(
                    "Plan materialization path or digest does not match its revision",
                ));
            }
            if state.plans.contains_key(plan) {
                let mut retryable = false;
                for materialization in state
                    .materializations
                    .values()
                    .filter(|materialization| materialization.plan == *plan)
                {
                    match &materialization.outcome {
                        Some(MaterializationOutcome::Written | MaterializationOutcome::Matched) => {
                            return Err(ReduceError::State(
                                "a usable Plan materialization cannot be retried",
                            ));
                        }
                        Some(
                            MaterializationOutcome::Conflict { .. }
                            | MaterializationOutcome::Failed { .. },
                        ) => {
                            retryable = true;
                        }
                        None => {}
                    }
                }
                if !retryable {
                    return Err(ReduceError::State(
                        "Plan materialization retry requires a conflict or visible failure",
                    ));
                }
            }
        }
        FactData::PlanMaterializationFinished { attempt, plan, .. } => {
            let Some(materialization) = state.materializations.get(attempt) else {
                return Err(unknown("Plan materialization"));
            };
            if state.open_materialization != Some(*attempt)
                || materialization.plan != *plan
                || materialization.outcome.is_some()
            {
                return Err(ReduceError::State(
                    "Plan materialization terminal does not match intent",
                ));
            }
        }
        FactData::ForkPlanned {
            intent,
            source_seq,
            source_digest,
            observation,
            plan_footprint,
            ..
        } => {
            if let Some(footprint) = plan_footprint {
                footprint.validate(&intent.path_component())?;
            }
            if state.operation.is_some()
                || state.open_fork.is_some()
                || *source_seq != state.seq
                || Some(*source_digest) != state.head
                || !known_observation(state, commit, *observation)
                || fork_blocked(state, commit)
                || state.forks.contains_key(intent)
            {
                return Err(ReduceError::State(
                    "fork intent is not based on a closed clean prefix",
                ));
            }
        }
        FactData::ForkCancelRequested { intent } => {
            if state.open_fork.as_ref().map(|fork| fork.id) != Some(*intent) {
                return Err(unknown("open fork intent"));
            }
        }
        FactData::ForkFinished { intent, outcome } => {
            if state.open_fork.as_ref().map(|fork| fork.id) != Some(*intent) {
                return Err(unknown("open fork intent"));
            }
            if let ForkOutcome::Published { observation, .. } = outcome
                && !known_observation(state, commit, *observation)
            {
                return Err(unknown("fork publication observation"));
            }
        }
        FactData::ForkedFrom { source_seq, .. } => {
            if state.session.is_some() || *source_seq == 0 {
                return Err(ReduceError::State(
                    "ForkedFrom is valid only in a child root",
                ));
            }
        }
        FactData::ForkSnapshotSealed { preceding, .. } => {
            if state.seq == 0 || Some(*preceding) != state.head {
                return Err(ReduceError::State(
                    "fork seal does not name the preceding child head",
                ));
            }
        }
        FactData::CommandRejected { .. } => {}
        FactData::InputAccepted { input, route, .. } => {
            if state.inputs.contains_key(input) {
                return Err(ReduceError::Duplicate { kind: "input" });
            }
            if *route == InputRoute::Start && state.operation.is_some() {
                return Err(ReduceError::State(
                    "start-routed input requires an idle session",
                ));
            }
        }
        FactData::InputRouted { input, .. } => {
            if !matches!(state.inputs.get(input), Some(InputState::Pending(_))) {
                return Err(ReduceError::State("only a pending input can change route"));
            }
        }
        FactData::InputApplied { input, .. } => {
            let accepted_here = commit.facts.iter().any(|fact| {
                matches!(&fact.data, FactData::InputAccepted { input: id, .. } if id == input)
            });
            if !accepted_here && !matches!(state.inputs.get(input), Some(InputState::Pending(_))) {
                return Err(ReduceError::State("only a pending input can be applied"));
            }
        }
        FactData::InputCancelled { input, .. } => {
            if !matches!(state.inputs.get(input), Some(InputState::Pending(_))) {
                return Err(ReduceError::State("only a pending input can be cancelled"));
            }
        }
        FactData::DirectiveChanged { directive } => directive_change(state, directive)?,
        FactData::WorkSet { work } => validate_work(state, commit, work)?,
        FactData::WorkspaceObserved { observation } => {
            validate_observation(state, commit, observation)?
        }
        FactData::OperationOpened {
            operation,
            work,
            stage,
            ..
        } => operation_open(state, commit, *operation, *work, *stage)?,
        FactData::AbortRequested { operation, .. } => {
            let Some(open) = &state.operation else {
                return Err(unknown("open operation"));
            };
            if open.id != *operation || open.aborting {
                return Err(ReduceError::State(
                    "abort does not match one productive operation",
                ));
            }
        }
        FactData::OperationClosed { close, .. } => operation_close(state, commit, close)?,
        FactData::ProviderStarted { intent } => provider_start(state, commit, intent)?,
        FactData::ProviderFinished {
            attempt,
            result,
            outcome,
        } => provider_finish(state, commit, *attempt, *result, outcome)?,
        FactData::CompletionCheckRecorded { .. } => {}
        FactData::ToolCallPlanned { call } => tool_plan(state, commit, call)?,
        FactData::ToolStarted { intent } => tool_start(state, intent)?,
        FactData::ToolFinished { result } => tool_finish(state, commit, result)?,
        FactData::DetachedJobOpened {
            job,
            attempt,
            process,
            contract_digest,
            ..
        } => {
            let intent = state
                .tool_attempts
                .get(attempt)
                .and_then(|call| state.tools.get(call))
                .and_then(|tool| tool.intent.as_ref());
            if state.jobs.contains_key(job) || process.job() != *job || intent.is_none() {
                return Err(ReduceError::State(
                    "detached job does not match its tool attempt",
                ));
            }
            process.validate(process.containment())?;
            let intent = intent.ok_or_else(|| unknown("detached tool intent"))?;
            if intent.reconciliation.digest()? != *contract_digest {
                return Err(ReduceError::State(
                    "detached job contract digest does not match tool intent",
                ));
            }
        }
        FactData::EffectObserved {
            observation,
            observing_result,
        } => effect_observed(state, commit, observation, *observing_result)?,
        FactData::DetachedJobClosed { job, outcome, .. } => {
            let Some(record) = state.jobs.get(job) else {
                return Err(unknown("detached job"));
            };
            if record.close.is_some() {
                return Err(ReduceError::State("detached job is already closed"));
            }
            if *outcome == DetachedClose::Terminal
                && !commit.facts.iter().any(|fact| matches!(&fact.data, FactData::EffectObserved { observation, .. } if observation.target == record.attempt && observation.resolution == ReconciliationResolution::TerminalKnown))
            {
                return Err(ReduceError::State(
                    "terminal detached close requires matching terminal observation",
                ));
            }
        }
        FactData::RetryScheduled { retry } => {
            if state.retry.is_some()
                || state.open_provider.is_some()
                || state.operation.as_ref().map(|open| open.id) != Some(retry.operation)
                || retry.not_before_unix_millis > retry.deadline_unix_millis
            {
                return Err(ReduceError::State(
                    "retry cannot start in the current state",
                ));
            }
        }
        FactData::RetryFinished { retry, .. } => {
            if state.retry != Some(*retry) {
                return Err(unknown("open retry"));
            }
        }
        FactData::ContextEpochOpened { epoch } => epoch_open(state, commit, epoch)?,
        FactData::ContextEntryAdded { entry } => entry_add(state, commit, entry)?,
        FactData::ConversationCleared {
            previous_epoch,
            replacement_epoch,
        } => {
            if state.operation.is_some()
                || state.current_epoch != Some(*previous_epoch)
                || !same_epoch(commit, *replacement_epoch)
            {
                return Err(ReduceError::State(
                    "clear does not replace the idle current epoch",
                ));
            }
        }
        FactData::MeterSettled { meter, owner, .. } => {
            if state.meters.settled.contains_key(meter) {
                return Err(ReduceError::Duplicate {
                    kind: "meter settlement",
                });
            }
            meter_owner(state, owner)?;
        }
    }
    Ok(())
}

fn validate_observation(
    state: &SessionState,
    commit: &Commit,
    observation: &WorkspaceObservation,
) -> Result<()> {
    observation.validate()?;
    if state.observations.contains_key(&observation.id()) {
        return Err(ReduceError::Duplicate {
            kind: "workspace observation",
        });
    }
    if !known_scope(state, commit, observation.scope()) {
        return Err(unknown("workspace scope"));
    }
    Ok(())
}

fn epoch_open(state: &SessionState, commit: &Commit, epoch: &ContextEpoch) -> Result<()> {
    if state.epochs.contains_key(&epoch.id) || epoch.head.calculated_digest()? != epoch.head.digest
    {
        return Err(ReduceError::State(
            "context epoch is duplicate or has a bad head digest",
        ));
    }
    let mut base = HashSet::new();
    for entry in epoch.base.iter() {
        if !base.insert(*entry) || !state.entries.contains_key(entry) {
            return Err(unknown("epoch base entry"));
        }
    }
    if state.seq > 0
        && state.operation.is_some()
        && !commit.facts.iter().any(|fact| {
            matches!(
                &fact.data,
                FactData::OperationClosed { .. }
                    | FactData::ProviderFinished {
                        outcome: ProviderOutcome::Failed { .. },
                        ..
                    }
            )
        })
    {
        let unfinished_tools = state.tools.values().any(|tool| tool.result.is_none());
        let safe = state.open_provider.is_none()
            && state.retry.is_none()
            && state.open_materialization.is_none()
            && state.open_fork.is_none()
            && !unfinished_tools;
        if !safe {
            return Err(ReduceError::State(
                "context rebase is not at a safe boundary",
            ));
        }
    }
    Ok(())
}

fn entry_add(state: &SessionState, commit: &Commit, entry: &ContextEntry) -> Result<()> {
    let added = commit
        .facts
        .iter()
        .filter(|fact| {
            matches!(&fact.data, FactData::ContextEntryAdded { entry: value } if value.epoch == entry.epoch)
        })
        .count();
    let current = state.epoch_entries.get(&entry.epoch).map_or(0, Vec::len);
    if state.entries.contains_key(&entry.id)
        || entry.digest != entry.calculated_digest()
        || (!state.epochs.contains_key(&entry.epoch) && !same_epoch(commit, entry.epoch))
        || (state.current_epoch != Some(entry.epoch) && !same_epoch(commit, entry.epoch))
        || current.saturating_add(added) > ENTRY_LIMIT
    {
        return Err(ReduceError::State(
            "context entry is duplicate, edited, or names an unknown epoch",
        ));
    }
    if !known_source(state, commit, &entry.source) {
        return Err(unknown("context source"));
    }
    Ok(())
}

fn directive_change(state: &SessionState, directive: &Directive) -> Result<()> {
    let Some(current) = state.directives.get(&directive.id) else {
        if !matches!(directive.state, DirectiveState::Active) {
            return Err(ReduceError::State("new directive must start active"));
        }
        return Ok(());
    };
    if current.text != directive.text || !matches!(current.state, DirectiveState::Active) {
        return Err(ReduceError::State(
            "directive text is immutable and terminal states do not reopen",
        ));
    }
    Ok(())
}

fn known_scope(state: &SessionState, commit: &Commit, id: WorkspaceScopeId) -> bool {
    state.scopes.contains_key(&id)
        || state
            .works
            .values()
            .filter_map(|work| work.verification())
            .any(|policy| policy.scope() == id)
        || commit.facts.iter().any(|fact| match &fact.data {
            FactData::SessionCreated { scope, .. } => scope.id() == id,
            FactData::WorkSet { work } => work
                .verification()
                .is_some_and(|policy| policy.scope() == id),
            _ => false,
        })
}

fn known_observation(state: &SessionState, commit: &Commit, id: ObservationId) -> bool {
    find_observation(state, commit, id).is_some()
}

fn find_observation<'a>(
    state: &'a SessionState,
    commit: &'a Commit,
    id: ObservationId,
) -> Option<&'a WorkspaceObservation> {
    state.observations.get(&id).or_else(|| {
        commit.facts.iter().find_map(|fact| match &fact.data {
            FactData::WorkspaceObserved { observation } if observation.id() == id => {
                Some(observation)
            }
            _ => None,
        })
    })
}

fn same_observation(commit: &Commit, id: ObservationId) -> bool {
    commit.facts.iter().any(|fact| {
        matches!(&fact.data, FactData::WorkspaceObserved { observation } if observation.id() == id)
    })
}

fn known_entry(state: &SessionState, commit: &Commit, id: ContextEntryId) -> bool {
    state.entries.contains_key(&id)
        || commit.facts.iter().any(
            |fact| matches!(&fact.data, FactData::ContextEntryAdded { entry } if entry.id == id),
        )
}

fn same_epoch(commit: &Commit, id: ContextEpochId) -> bool {
    commit
        .facts
        .iter()
        .any(|fact| matches!(&fact.data, FactData::ContextEpochOpened { epoch } if epoch.id == id))
}

fn known_source(state: &SessionState, commit: &Commit, source: &ContextSource) -> bool {
    match source {
        ContextSource::Input(id) | ContextSource::WorkPolicy(id) => {
            state.inputs.contains_key(id)
                || commit.facts.iter().any(|fact| matches!(&fact.data, FactData::InputAccepted { input, .. } if input == id))
        }
        ContextSource::Provider(id) | ContextSource::Receipt(id) | ContextSource::Summary(id) => {
            state.providers.values().any(|provider| match provider {
                ProviderState::Started(intent) | ProviderState::Finished { intent, .. } => {
                    intent.result == *id
                }
            }) || commit.facts.iter().any(|fact| matches!(&fact.data, FactData::ProviderFinished { result, .. } if result == id))
        }
        ContextSource::Tool(id) => {
            state.tool_results.contains(id)
                || commit.facts.iter().any(|fact| matches!(&fact.data, FactData::ToolFinished { result } if result.id == *id))
        }
        ContextSource::Plan(id) => find_plan(state, commit, *id).is_some(),
        ContextSource::Work(id) => find_work(state, commit, *id).is_some(),
        ContextSource::Directive(id) => {
            state.directives.contains_key(id)
                || commit.facts.iter().any(|fact| matches!(&fact.data, FactData::DirectiveChanged { directive } if directive.id == *id))
        }
        ContextSource::Evidence(id) => {
            state.evidence.contains(id)
                || commit.facts.iter().any(|fact| {
                    matches!(&fact.data, FactData::WorkSet { work } if work.evidence.iter().any(|evidence| evidence.id == *id))
                })
        }
        ContextSource::HarnessControl => true,
    }
}

fn unknown(kind: &'static str) -> ReduceError {
    ReduceError::Unknown { kind }
}
