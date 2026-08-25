use std::collections::HashSet;

use super::{ReduceError, Result, SessionState};
use crate::durable::context::ContextSource;
use crate::durable::encoding::Commit;
use crate::durable::fact::{FactData, MeterOwner};
use crate::durable::id::{ScopedId, SessionId};
use crate::durable::work::{PlanRevision, PlanSource, WorkProjection, WorkState, WorkStatus};
use crate::durable::workspace::WorkspaceScope;

pub(super) fn session(state: &SessionState, commit: &Commit) -> Result<SessionId> {
    if let Some(session) = state.session {
        return Ok(session);
    }
    commit
        .facts
        .iter()
        .find_map(|fact| match &fact.data {
            FactData::SessionCreated { session, .. } => Some(*session),
            _ => None,
        })
        .ok_or(ReduceError::State("first commit does not create a session"))
}

pub(super) fn validate(state: &SessionState, commit: &Commit, session: SessionId) -> Result<()> {
    local(commit.id, session, "commit")?;
    if let Some(receipt) = &commit.command {
        local(receipt.id, session, "command")?;
        if state.receipt(receipt.id).is_some() {
            return Err(ReduceError::Duplicate { kind: "command" });
        }
    }
    let mut fact_ids = HashSet::new();
    for fact in &commit.facts {
        local(fact.id, session, "fact")?;
        if state.facts.contains_key(&fact.id) || !fact_ids.insert(fact.id) {
            return Err(ReduceError::Duplicate { kind: "fact" });
        }
        fact_data(&fact.data, session)?;
    }
    Ok(())
}

fn fact_data(fact: &FactData, session: SessionId) -> Result<()> {
    match fact {
        FactData::SessionCreated {
            session: created,
            scope,
        } => {
            if *created != session {
                return Err(namespace("created session", session));
            }
            workspace_scope(scope, session)?;
        }
        FactData::BudgetChanged { .. } => {}
        FactData::PlanSet { plan } => plan_revision(plan, session)?,
        FactData::PlanMaterializationPlanned { attempt, plan, .. }
        | FactData::PlanMaterializationFinished { attempt, plan, .. } => {
            local(*attempt, session, "Plan materialization")?;
            local(*plan, session, "Plan revision")?;
        }
        FactData::ForkPlanned {
            intent,
            observation,
            ..
        } => {
            local(*intent, session, "fork intent")?;
            local(*observation, session, "fork observation")?;
        }
        FactData::ForkFinished {
            intent,
            observation,
            ..
        } => {
            local(*intent, session, "fork intent")?;
            local(*observation, session, "fork publication observation")?;
        }
        FactData::ForkedFrom { .. } | FactData::ForkSnapshotSealed { .. } => {
            // Fork provenance is the one deliberate cross-session ID reference.
        }
        FactData::CommandRejected { .. } => {}
        FactData::InputAccepted { input, .. } => local(*input, session, "input")?,
        FactData::InputApplied {
            input, operation, ..
        } => {
            local(*input, session, "input")?;
            local(*operation, session, "input operation")?;
        }
        FactData::WorkSet { work } => work_state(work, session)?,
        FactData::WorkspaceObserved { observation } => {
            local(observation.id, session, "observation")?;
            local(observation.scope, session, "observation scope")?;
        }
        FactData::OperationOpened {
            operation,
            input,
            work,
            ..
        } => {
            local(*operation, session, "operation")?;
            local(*input, session, "operation input")?;
            optional(*work, session, "operation Work")?;
        }
        FactData::AbortRequested { operation, .. } => {
            local(*operation, session, "operation")?;
        }
        FactData::OperationClosed { close, .. } => {
            local(close.id, session, "operation")?;
            optional(close.provider, session, "provider result")?;
            optional(close.work, session, "Work")?;
            optional(close.plan, session, "Plan")?;
            optional(close.candidate, session, "candidate")?;
            optional(close.verdict, session, "verdict")?;
        }
        FactData::ProviderStarted { intent } => {
            local(intent.attempt, session, "provider attempt")?;
            local(intent.result, session, "provider result")?;
            local(intent.operation, session, "provider operation")?;
            local(intent.epoch, session, "provider epoch")?;
            optional(intent.through, session, "provider through-entry")?;
        }
        FactData::ProviderFinished {
            attempt, result, ..
        } => {
            local(*attempt, session, "provider attempt")?;
            local(*result, session, "provider result")?;
        }
        FactData::CompletionCheckRecorded {
            operation, check, ..
        } => {
            local(*operation, session, "completion check operation")?;
            if let crate::durable::fact::CompletionCheck::Declared(check) = check {
                local(*check, session, "completion check")?;
            }
        }
        FactData::ToolCallPlanned { call } => {
            local(call.id, session, "tool call")?;
            local(call.batch, session, "tool batch")?;
            local(call.result, session, "tool result")?;
            local(call.provider_result, session, "tool provider result")?;
        }
        FactData::ToolStarted { intent } => {
            local(intent.attempt, session, "tool attempt")?;
            local(intent.call, session, "tool call")?;
        }
        FactData::ToolFinished { result } => {
            local(result.id, session, "tool result")?;
            local(result.call, session, "tool call")?;
            optional(result.attempt, session, "tool attempt")?;
        }
        FactData::ContextEpochOpened { epoch } => {
            local(epoch.id, session, "context epoch")?;
            many(epoch.base.iter().copied(), session, "epoch base entry")?;
        }
        FactData::ContextEntryAdded { entry } => {
            local(entry.id, session, "context entry")?;
            local(entry.epoch, session, "context epoch")?;
            context_source(&entry.source, session)?;
        }
        FactData::ConversationCleared {
            previous_epoch,
            replacement_epoch,
        } => {
            local(*previous_epoch, session, "previous epoch")?;
            local(*replacement_epoch, session, "replacement epoch")?;
        }
        FactData::MeterSettled { owner, .. } => match owner {
            MeterOwner::Provider(id) => local(*id, session, "meter provider")?,
            MeterOwner::Tool(id) => local(*id, session, "meter tool")?,
        },
    }
    Ok(())
}

fn plan_revision(plan: &PlanRevision, session: SessionId) -> Result<()> {
    local(plan.id, session, "Plan")?;
    local(plan.work, session, "Plan Work")?;
    match plan.source {
        PlanSource::Generated { provider } => local(provider, session, "Plan provider")?,
        PlanSource::External => {}
    }
    for step in plan.steps.iter() {
        local(step.id, session, "Plan step")?;
    }
    many(
        plan.checks.iter().map(|check| check.id),
        session,
        "Plan check",
    )?;
    Ok(())
}

fn work_state(work: &WorkState, session: SessionId) -> Result<()> {
    local(work.id, session, "Work")?;
    optional(work.selected_plan, session, "selected Plan")?;
    optional(work.active_item, session, "active Work item")?;
    optional(work.scope, session, "Work scope")?;
    optional(work.base, session, "Work base")?;
    many(
        work.acceptance_checks.iter().copied(),
        session,
        "Work check",
    )?;
    for evidence in work.evidence.iter() {
        local(evidence.id, session, "Work evidence")?;
        local(evidence.source, session, "Work evidence source")?;
    }
    for item in work.items.iter() {
        local(item.id, session, "Work item")?;
        many(item.evidence.iter().copied(), session, "Work item evidence")?;
    }
    match &work.status {
        WorkStatus::Completed { candidate, verdict } => {
            local(*candidate, session, "completed candidate")?;
            local(*verdict, session, "completed verdict")?;
        }
        WorkStatus::Active(_) | WorkStatus::Abandoned { .. } => {}
    }
    if let Some(candidate) = &work.candidate {
        local(candidate.id, session, "candidate")?;
        local(candidate.work, session, "candidate Work")?;
        local(candidate.plan, session, "candidate Plan")?;
        local(candidate.policy, session, "candidate policy")?;
        local(candidate.scope, session, "candidate scope")?;
        local(candidate.base, session, "candidate base")?;
        local(candidate.capture, session, "candidate capture")?;
    }
    if let Some(policy) = &work.verification {
        local(policy.id, session, "verification policy")?;
        local(policy.scope, session, "verification scope")?;
        many(policy.checks.iter().copied(), session, "verification check")?;
    }
    match work.projection_owner {
        WorkProjection::Context(id) => local(id, session, "Work context")?,
        WorkProjection::Epoch(id) => local(id, session, "Work epoch")?,
        WorkProjection::Pending(id) => local(id, session, "Work batch")?,
        WorkProjection::CompletionCheck(id) => {
            local(id, session, "Work completion check")?;
        }
        WorkProjection::Operation(id) => local(id, session, "Work operation")?,
        WorkProjection::Plan(id) => local(id, session, "Work Plan projection")?,
    }
    Ok(())
}

fn workspace_scope(scope: &WorkspaceScope, session: SessionId) -> Result<()> {
    local(scope.id, session, "workspace scope")?;
    for artifact in scope.artifacts.iter() {
        local(artifact.rule, session, "workspace artifact rule")?;
    }
    Ok(())
}

fn context_source(source: &ContextSource, session: SessionId) -> Result<()> {
    match source {
        ContextSource::Input(id) | ContextSource::WorkPolicy(id) => {
            local(*id, session, "context input")
        }
        ContextSource::Provider(id) | ContextSource::Receipt(id) => {
            local(*id, session, "context provider")
        }
        ContextSource::Tool(id) => local(*id, session, "context tool"),
        ContextSource::Plan(id) => local(*id, session, "context Plan"),
        ContextSource::Work(id) => local(*id, session, "context Work"),
        ContextSource::HarnessControl => Ok(()),
    }
}

fn many<K>(
    values: impl IntoIterator<Item = ScopedId<K>>,
    session: SessionId,
    field: &'static str,
) -> Result<()> {
    for value in values {
        local(value, session, field)?;
    }
    Ok(())
}

fn optional<K>(value: Option<ScopedId<K>>, session: SessionId, field: &'static str) -> Result<()> {
    if let Some(value) = value {
        local(value, session, field)?;
    }
    Ok(())
}

fn local<K>(value: ScopedId<K>, session: SessionId, field: &'static str) -> Result<()> {
    if !value.belongs(session) {
        return Err(namespace(field, session));
    }
    Ok(())
}

fn namespace(field: &'static str, session: SessionId) -> ReduceError {
    ReduceError::Namespace { field, session }
}
