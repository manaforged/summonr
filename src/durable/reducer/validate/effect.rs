use std::collections::HashSet;

use super::work::{find_work, materialized};
use super::{known_entry, unknown};
use crate::durable::context::{ContextEpoch, ContextRole, ContextSource, OperationClose};
use crate::durable::effect::{
    DetachedClose, EffectObservation, ProcessRecord, ProviderIntent, ProviderOutcome,
    ProviderPurpose, ReconciliationContract, ReconciliationEvidence, ReconciliationResolution,
    StopReason, ToolCall, ToolIntent, ToolOutcome, ToolResult, WorkspaceFootprint, validate_calls,
};
use crate::durable::encoding::Commit;
use crate::durable::fact::{FactData, MeterOwner};
use crate::durable::id::{
    JobId, OperationId, ProviderAttemptId, ProviderResultId, ToolAttemptId, ToolCallId,
    ToolResultId, WorkId,
};
use crate::durable::reducer::{ProviderState, ReduceError, Result, SessionState};
use crate::durable::work::{Candidate, PlanSource, WorkStage, WorkStatus};
use crate::durable::workspace::{WorkspaceEffect, validate_paths};

pub(super) fn operation_open(
    state: &SessionState,
    commit: &Commit,
    operation: OperationId,
    work: Option<WorkId>,
    stage: Option<WorkStage>,
) -> Result<()> {
    if state.operation.is_some()
        || state.open_fork.is_some()
        || state.open_materialization.is_some()
        || state.closed_operation(operation).is_some()
    {
        return Err(ReduceError::State("operation cannot open at this boundary"));
    }
    match (work, stage) {
        (Some(work), Some(stage)) => {
            let work = find_work(state, commit, work).ok_or_else(|| unknown("operation Work"))?;
            if work.status().stage() != Some(stage)
                || !matches!(work.status(), WorkStatus::Active(_))
            {
                return Err(ReduceError::State(
                    "operation stage does not match active Work",
                ));
            }
            if matches!(stage, WorkStage::Execute | WorkStage::Verify)
                && !materialized(
                    state,
                    commit,
                    work.selected_plan()
                        .ok_or(ReduceError::State("stage has no Plan"))?,
                )
            {
                return Err(ReduceError::State("selected Plan is not materialized"));
            }
        }
        (None, None) => {}
        _ => {
            return Err(ReduceError::State(
                "operation Work and stage must appear together",
            ));
        }
    }
    Ok(())
}

pub(super) fn operation_close(
    state: &SessionState,
    commit: &Commit,
    close: &OperationClose,
) -> Result<()> {
    let Some(open) = &state.operation else {
        return Err(unknown("open operation"));
    };
    let provider_finishes = state.open_provider.is_some_and(|attempt| {
        commit.facts.iter().any(|fact| {
            matches!(&fact.data, FactData::ProviderFinished { attempt: id, .. } if *id == attempt)
        })
    });
    let retry_finishes = state.retry.is_some_and(|retry| {
        commit.facts.iter().any(
            |fact| matches!(&fact.data, FactData::RetryFinished { retry: id, .. } if *id == retry),
        )
    });
    if open.id != close.id
        || (state.open_provider.is_some() && !provider_finishes)
        || (state.retry.is_some() && !retry_finishes)
        || unresolved_tools(state, commit)
    {
        return Err(ReduceError::State(
            "operation close has unresolved or mismatched obligations",
        ));
    }
    if close.work != open.work {
        return Err(ReduceError::State(
            "operation close Work does not match its open",
        ));
    }
    if let Some(work_id) = close.work {
        let work = find_work(state, commit, work_id).ok_or_else(|| unknown("close Work"))?;
        if close.work_status.as_ref() != Some(work.status())
            || close.plan != work.selected_plan()
            || close.candidate != work.candidate().map(Candidate::id)
        {
            return Err(ReduceError::State(
                "operation close does not match its atomic Work revision",
            ));
        }
    }
    Ok(())
}

pub(super) fn provider_start(
    state: &SessionState,
    commit: &Commit,
    intent: &ProviderIntent,
) -> Result<()> {
    intent.validate()?;
    let Some(operation) = &state.operation else {
        return Err(unknown("provider operation"));
    };
    let closes_retry = state.retry.is_some_and(|retry| {
        commit.facts.iter().any(
            |fact| matches!(&fact.data, FactData::RetryFinished { retry: id, .. } if *id == retry),
        )
    });
    if operation.id != intent.operation
        || operation.aborting
        || state.open_provider.is_some()
        || (state.retry.is_some() && !closes_retry)
        || unresolved_tools(state, commit)
        || state.providers.contains_key(&intent.attempt)
        || state.providers.values().any(|provider| match provider {
            ProviderState::Started(known) | ProviderState::Finished { intent: known, .. } => {
                known.result == intent.result
            }
        })
        || state.current_epoch != Some(intent.epoch)
    {
        return Err(ReduceError::State(
            "provider attempt cannot start in the current state",
        ));
    }
    let epoch = state
        .epochs
        .get(&intent.epoch)
        .ok_or_else(|| unknown("provider context epoch"))?;
    if !head_matches(intent, epoch) {
        return Err(ReduceError::State(
            "provider request identity does not match its frozen context epoch",
        ));
    }
    if intent.work != operation.work
        || intent.stage != operation.stage
        || epoch.stage != operation.stage
    {
        return Err(ReduceError::State(
            "provider stage does not match its operation",
        ));
    }
    match operation.work {
        Some(work_id) => {
            let work = find_work(state, commit, work_id).ok_or_else(|| unknown("provider Work"))?;
            if intent.plan != work.selected_plan()
                || intent.candidate != work.candidate().map(Candidate::id)
            {
                return Err(ReduceError::State(
                    "provider Plan or candidate does not match current Work",
                ));
            }
            if intent.stage == Some(WorkStage::Verify)
                && work.verification().is_none_or(|policy| {
                    policy.reviewer.endpoint.as_str() != intent.endpoint.as_str()
                        || policy.reviewer.model.as_str() != intent.model.as_str()
                        || policy.reviewer.request_format != intent.request_format
                })
            {
                return Err(ReduceError::State(
                    "Verify provider does not match the selected review head",
                ));
            }
        }
        None if intent.plan.is_some() || intent.candidate.is_some() => {
            return Err(ReduceError::State(
                "provider without Work cannot name a Plan or candidate",
            ));
        }
        None => {}
    }
    let latest = commit
        .facts
        .iter()
        .rev()
        .find_map(|fact| match &fact.data {
            FactData::ContextEntryAdded { entry } if entry.epoch == intent.epoch => Some(entry.id),
            _ => None,
        })
        .or_else(|| {
            state
                .epoch_entries
                .get(&intent.epoch)
                .and_then(|entries| entries.last().copied())
        });
    if intent.purpose == ProviderPurpose::AgentRound && intent.through != latest {
        return Err(ReduceError::State(
            "agent round does not extend through the latest immutable context entry",
        ));
    }
    if intent
        .through
        .is_some_and(|through| !known_entry(state, commit, through))
    {
        return Err(unknown("provider through-entry"));
    }
    Ok(())
}

fn head_matches(intent: &ProviderIntent, epoch: &ContextEpoch) -> bool {
    intent.adapter == epoch.head.adapter
        && intent.endpoint == epoch.head.endpoint
        && intent.model == epoch.head.model
        && intent.resource_version == epoch.head.resource_version
        && intent.request_format == epoch.head.request_format
}

pub(super) fn provider_finish(
    state: &SessionState,
    commit: &Commit,
    attempt: ProviderAttemptId,
    result: ProviderResultId,
    outcome: &ProviderOutcome,
) -> Result<()> {
    let Some(ProviderState::Started(intent)) = state.providers.get(&attempt) else {
        return Err(unknown("open provider attempt"));
    };
    if state.open_provider != Some(attempt) || intent.result != result {
        return Err(ReduceError::State(
            "provider result does not match its intent",
        ));
    }
    if let ProviderOutcome::Completed {
        message,
        stop_reason,
        ..
    } = outcome
    {
        if intent.purpose == ProviderPurpose::AgentRound {
            let assistant_entries = commit
                .facts
                .iter()
                .filter(|fact| {
                    matches!(
                        &fact.data,
                        FactData::ContextEntryAdded { entry }
                            if entry.epoch == intent.epoch
                                && entry.role == ContextRole::Assistant
                                && entry.source == ContextSource::Provider(result)
                    )
                })
                .count();
            if assistant_entries != 1 {
                return Err(ReduceError::State(
                    "completed agent round requires one assistant context entry",
                ));
            }
        }
        let calls: Vec<_> = commit
            .facts
            .iter()
            .filter_map(|fact| match &fact.data {
                FactData::ToolCallPlanned { call } if call.provider_result == result => Some(call),
                _ => None,
            })
            .collect();
        if calls.len() != message.tool_calls.len() {
            return Err(ReduceError::State(
                "provider tool-call payload and planned obligations differ",
            ));
        }
        if calls
            .first()
            .is_some_and(|first| calls.iter().any(|call| call.batch != first.batch))
        {
            return Err(ReduceError::State(
                "one provider result cannot create more than one tool batch",
            ));
        }
        for (position, (call, payload)) in calls.iter().zip(message.tool_calls.iter()).enumerate() {
            if usize::from(call.position) != position || call.provider_id != payload.provider_id {
                return Err(ReduceError::State(
                    "provider tool-call order or identity changed",
                ));
            }
        }
        validate_calls(&calls)?;
        if intent.stage == Some(WorkStage::Verify) && *stop_reason == StopReason::Complete {
            let reviewed = intent.work.is_some_and(|work| {
                commit.facts.iter().any(
                    |fact| matches!(&fact.data, FactData::WorkSet { work: value } if value.id() == work),
                )
            });
            let observed = commit
                .facts
                .iter()
                .any(|fact| matches!(fact.data, FactData::WorkspaceObserved { .. }));
            let closed = commit.facts.iter().any(|fact| {
                matches!(&fact.data, FactData::OperationClosed { close, .. }
                    if close.id == intent.operation && close.provider == Some(result))
            });
            if !reviewed || !observed || !closed {
                return Err(ReduceError::State(
                    "completed Verify review requires candidate observation, Work, and operation close",
                ));
            }
        }
        if intent.stage == Some(WorkStage::Plan) && *stop_reason == StopReason::Complete {
            let plan = commit.facts.iter().find_map(|fact| match &fact.data {
                FactData::PlanSet { plan }
                    if intent.work == Some(plan.work())
                        && matches!(plan.source, PlanSource::Generated { provider } if provider == result) =>
                {
                    Some(plan.id())
                }
                _ => None,
            });
            let planned = plan.is_some_and(|plan| {
                commit.facts.iter().any(|fact| {
                    matches!(&fact.data, FactData::PlanMaterializationPlanned { plan: id, .. } if *id == plan)
                })
            });
            let work = intent.work.is_some_and(|work| {
                commit.facts.iter().any(
                    |fact| matches!(&fact.data, FactData::WorkSet { work: value } if value.id() == work),
                )
            });
            let closed = commit.facts.iter().any(|fact| {
                matches!(&fact.data, FactData::OperationClosed { close, .. }
                    if close.id == intent.operation && close.provider == Some(result))
            });
            if plan.is_none() || !planned || !work || !closed {
                return Err(ReduceError::State(
                    "completed Plan reply requires Plan, publication intent, Work, and operation close",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn tool_plan(state: &SessionState, commit: &Commit, call: &ToolCall) -> Result<()> {
    if state.tools.contains_key(&call.id)
        || state.batches.contains_key(&call.batch)
        || state.tool_results.contains(&call.result)
        || commit.facts.iter().any(|fact| {
            matches!(&fact.data, FactData::ToolCallPlanned { call: other } if other.id != call.id && (other.result == call.result || other.provider_id == call.provider_id))
        })
    {
        return Err(ReduceError::Duplicate {
            kind: "tool call or result",
        });
    }
    Ok(())
}

pub(super) fn tool_start(state: &SessionState, intent: &ToolIntent) -> Result<()> {
    let Some(tool) = state.tools.get(&intent.call) else {
        return Err(unknown("planned tool call"));
    };
    if tool.intent.is_some()
        || tool.result.is_some()
        || state.tool_attempts.contains_key(&intent.attempt)
        || state
            .operation
            .as_ref()
            .is_none_or(|operation| operation.aborting)
        || tool.call.registration != intent.registration
        || tool.call.effect_contract_digest != intent.reconciliation.digest()?
    {
        return Err(ReduceError::State(
            "tool intent does not match its planned call",
        ));
    }
    if let Some(process) = &intent.process {
        process.validate(process.containment())?;
        if let ReconciliationContract::Process { job } = intent.reconciliation
            && process.job() != job
        {
            return Err(ReduceError::State(
                "process reconciliation job does not match",
            ));
        }
    }
    if let WorkspaceFootprint::Exact(paths) = &intent.footprint {
        validate_paths(paths.as_slice())?;
    }
    Ok(())
}

pub(super) fn tool_finish(
    state: &SessionState,
    commit: &Commit,
    result: &ToolResult,
) -> Result<()> {
    let Some(tool) = state.tools.get(&result.call) else {
        return Err(unknown("planned tool call"));
    };
    if tool.result.is_some()
        || result.id != tool.call.result
        || state.tool_results.contains(&result.id)
    {
        return Err(ReduceError::State(
            "tool call already has a result or result ID changed",
        ));
    }
    result.workspace.validate()?;
    match (&tool.intent, result.outcome.needs_start()) {
        (Some(intent), true) if result.attempt == Some(intent.attempt) => {
            match (&intent.footprint, &result.workspace) {
                (WorkspaceFootprint::None, WorkspaceEffect::None) => {}
                (WorkspaceFootprint::None, _) => {
                    return Err(ReduceError::State(
                        "None workspace footprint cannot report a workspace change",
                    ));
                }
                (WorkspaceFootprint::Exact(paths), WorkspaceEffect::Known { changes, .. }) => {
                    if changes.paths().any(|path| !paths.as_slice().contains(path)) {
                        return Err(ReduceError::State(
                            "exact tool effect changed a path outside its footprint",
                        ));
                    }
                }
                (
                    WorkspaceFootprint::Exact(_),
                    WorkspaceEffect::None | WorkspaceEffect::Unknown { .. },
                )
                | (WorkspaceFootprint::Scope(_), _) => {}
            }
        }
        (None, false)
            if result.attempt.is_none() && matches!(result.workspace, WorkspaceEffect::None) => {}
        _ => {
            return Err(ReduceError::State(
                "tool terminal start state is inconsistent",
            ));
        }
    }
    if let ToolOutcome::Completed { job: Some(job), .. } = result.outcome
        && !commit.facts.iter().any(
            |fact| matches!(&fact.data, FactData::DetachedJobOpened { job: id, .. } if *id == job),
        )
    {
        return Err(ReduceError::State(
            "background result does not open its job",
        ));
    }
    Ok(())
}

pub(super) fn effect_observed(
    state: &SessionState,
    commit: &Commit,
    observation: &EffectObservation,
    observing_result: ToolResultId,
) -> Result<()> {
    let Some(call) = state.tool_attempts.get(&observation.target) else {
        return Err(unknown("target tool attempt"));
    };
    let Some(intent) = state.tools.get(call).and_then(|tool| tool.intent.as_ref()) else {
        return Err(unknown("target reconciliation contract"));
    };
    if intent.reconciliation.digest()? != observation.contract_digest {
        return Err(ReduceError::State("reconciliation contract digest changed"));
    }
    match (&intent.reconciliation, &observation.basis) {
        (
            ReconciliationContract::External {
                observer,
                rule,
                subject,
            },
            ReconciliationEvidence::External {
                observer: observed,
                rule: observed_rule,
                subject: observed_subject,
            },
        ) if observer == observed && rule == observed_rule && subject == observed_subject => {}
        (
            ReconciliationContract::Process { job },
            ReconciliationEvidence::Process {
                job: observed,
                record,
            },
        ) if job == observed && record.job() == *job => {
            record.validate(record.containment())?;
            if observation.resolution == ReconciliationResolution::TerminalKnown
                && !matches!(record, ProcessRecord::Terminal { .. })
            {
                return Err(ReduceError::State(
                    "terminal process reconciliation requires a terminal process record",
                ));
            }
        }
        _ => {
            return Err(ReduceError::State(
                "reconciliation observer, rule, or subject does not match",
            ));
        }
    }
    if !commit.facts.iter().any(|fact| {
        matches!(&fact.data, FactData::ToolFinished { result } if result.id == observing_result)
    }) {
        return Err(unknown("observing tool result"));
    }
    Ok(())
}

pub(super) fn meter_owner(state: &SessionState, owner: &MeterOwner) -> Result<()> {
    let known = match owner {
        MeterOwner::Provider(id) => {
            matches!(state.providers.get(id), Some(ProviderState::Started(_)))
        }
        MeterOwner::Tool(id) => state.tool_attempts.contains_key(id),
        MeterOwner::Retry(id) => state.retry == Some(*id),
    };
    if !known {
        return Err(unknown("meter owner"));
    }
    Ok(())
}

pub(super) fn tool_batch(state: &SessionState, commit: &Commit) -> Result<()> {
    let touched: HashSet<_> = commit
        .facts
        .iter()
        .filter_map(|fact| match &fact.data {
            FactData::ToolFinished { result } => {
                state.tools.get(&result.call).map(|tool| tool.call.batch)
            }
            _ => None,
        })
        .collect();
    let projected: Vec<_> = commit
        .facts
        .iter()
        .filter_map(|fact| match &fact.data {
            FactData::ContextEntryAdded { entry } => match entry.source {
                ContextSource::Tool(result) => Some(result),
                _ => None,
            },
            _ => None,
        })
        .collect();
    if projected.iter().any(|result| {
        state
            .tools
            .values()
            .find(|tool| tool.call.result == *result)
            .is_none_or(|tool| !touched.contains(&tool.call.batch))
    }) {
        return Err(ReduceError::State(
            "tool context can appear only in its terminal batch commit",
        ));
    }
    for batch in touched {
        let Some(calls) = state.batches.get(&batch) else {
            continue;
        };
        let result_for = |call: ToolCallId| -> Option<ToolResultId> {
            commit
                .facts
                .iter()
                .find_map(|fact| match &fact.data {
                    FactData::ToolFinished { result } if result.call == call => Some(result.id),
                    _ => None,
                })
                .or_else(|| {
                    state
                        .tools
                        .get(&call)
                        .and_then(|tool| tool.result.as_ref().map(|result| result.id))
                })
        };
        let expected: Vec<_> = calls.iter().filter_map(|call| result_for(*call)).collect();
        let actual: Vec<_> = projected
            .iter()
            .copied()
            .filter(|result| calls.iter().any(|call| result_for(*call) == Some(*result)))
            .collect();
        if expected.len() == calls.len() && actual != expected {
            return Err(ReduceError::State(
                "terminal tool batch must project every result in provider-call order",
            ));
        }
        if expected.len() != calls.len() && !actual.is_empty() {
            return Err(ReduceError::State(
                "partial tool batch cannot enter provider context",
            ));
        }
    }
    Ok(())
}

fn unresolved_tools(state: &SessionState, commit: &Commit) -> bool {
    state.tools.values().any(|tool| {
        tool.result.is_none()
            && !commit.facts.iter().any(|fact| {
                matches!(&fact.data, FactData::ToolFinished { result } if result.call == tool.call.id)
            })
    })
}

pub(super) fn work_blocked(state: &SessionState, commit: &Commit, work: WorkId) -> bool {
    state
        .jobs
        .iter()
        .any(|(id, job)| match close_here(commit, *id).or(job.close) {
            None => true,
            Some(DetachedClose::Terminal) => false,
            Some(DetachedClose::AbandonedUnknown) => job.work == Some(work),
        })
        || commit
            .facts
            .iter()
            .any(|fact| matches!(&fact.data, FactData::DetachedJobOpened { .. }))
        || state
            .uncertain_effects
            .iter()
            .any(|(attempt, effect)| effect.work == Some(work) && !resolved_here(commit, *attempt))
        || commit.facts.iter().any(|fact| match &fact.data {
            FactData::ToolFinished { result } if result.outcome.uncertain() => result
                .attempt
                .is_some_and(|attempt| state.attempt_work(attempt) == Some(work)),
            _ => false,
        })
}

pub(super) fn fork_blocked(state: &SessionState, commit: &Commit) -> bool {
    state
        .jobs
        .iter()
        .any(|(id, job)| match close_here(commit, *id).or(job.close) {
            None => true,
            Some(DetachedClose::Terminal) => false,
            Some(DetachedClose::AbandonedUnknown) => owner_active(state, commit, job.work),
        })
        || commit
            .facts
            .iter()
            .any(|fact| matches!(&fact.data, FactData::DetachedJobOpened { .. }))
        || state.uncertain_effects.iter().any(|(attempt, effect)| {
            !resolved_here(commit, *attempt) && owner_active(state, commit, effect.work)
        })
        || commit.facts.iter().any(|fact| match &fact.data {
            FactData::ToolFinished { result } if result.outcome.uncertain() => result
                .attempt
                .is_some_and(|attempt| owner_active(state, commit, state.attempt_work(attempt))),
            _ => false,
        })
}

fn owner_active(state: &SessionState, commit: &Commit, work: Option<WorkId>) -> bool {
    work.is_none_or(|id| find_work(state, commit, id).is_none_or(|work| !work.status().terminal()))
}

fn close_here(commit: &Commit, job: JobId) -> Option<DetachedClose> {
    commit.facts.iter().find_map(|fact| match &fact.data {
        FactData::DetachedJobClosed {
            job: id, outcome, ..
        } if *id == job => Some(*outcome),
        _ => None,
    })
}

fn resolved_here(commit: &Commit, attempt: ToolAttemptId) -> bool {
    commit.facts.iter().any(|fact| {
        matches!(
            &fact.data,
            FactData::EffectObserved { observation, .. }
                if observation.target == attempt
                    && observation.resolution == ReconciliationResolution::TerminalKnown
        )
    })
}
