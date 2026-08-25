use std::collections::HashSet;

use super::{ReduceError, Result, SessionState};
use crate::durable::context::{ContextSource, DirectiveState, InputDisposition};
use crate::durable::effect::{
    ApprovalSource, ReconciliationContract, ReconciliationEvidence, WorkspaceFootprint,
};
use crate::durable::encoding::Commit;
use crate::durable::fact::{FactData, ForkOutcome, MeterOwner};
use crate::durable::id::{ScopedId, SessionId};
use crate::durable::work::{PlanRevision, PlanSource, WorkProjection, WorkState, WorkStatus};
use crate::durable::workspace::{WorkspaceEffect, WorkspaceScope};

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
        FactData::ForkCancelRequested { intent } | FactData::ForkFinished { intent, .. } => {
            local(*intent, session, "fork intent")?;
            if let FactData::ForkFinished {
                outcome: ForkOutcome::Published { observation, .. },
                ..
            } = fact
            {
                local(*observation, session, "fork publication observation")?;
            }
        }
        FactData::ForkedFrom { .. } | FactData::ForkSnapshotSealed { .. } => {
            // Fork provenance is the one deliberate cross-session ID reference.
        }
        FactData::CommandRejected { .. } => {}
        FactData::InputAccepted { input, .. }
        | FactData::InputRouted { input, .. }
        | FactData::InputCancelled { input, .. } => local(*input, session, "input")?,
        FactData::InputApplied {
            input, operation, ..
        } => {
            local(*input, session, "input")?;
            optional(*operation, session, "input operation")?;
        }
        FactData::DirectiveChanged { directive } => {
            local(directive.id, session, "directive")?;
            match directive.state {
                DirectiveState::Satisfied { evidence } => {
                    optional(evidence, session, "directive evidence")?
                }
                DirectiveState::Superseded { by } => local(by, session, "directive successor")?,
                DirectiveState::Active => {}
            }
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
            for input in close.inputs.iter() {
                let id = match input {
                    InputDisposition::Applied(id)
                    | InputDisposition::Queued(id)
                    | InputDisposition::Cancelled(id) => *id,
                };
                local(id, session, "input disposition")?;
            }
        }
        FactData::ProviderStarted { intent } => {
            local(intent.attempt, session, "provider attempt")?;
            local(intent.result, session, "provider result")?;
            local(intent.operation, session, "provider operation")?;
            local(intent.epoch, session, "provider epoch")?;
            optional(intent.work, session, "provider Work")?;
            optional(intent.plan, session, "provider Plan")?;
            optional(intent.candidate, session, "provider candidate")?;
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
            local(call.registration, session, "tool registration")?;
        }
        FactData::ToolStarted { intent } => {
            local(intent.attempt, session, "tool attempt")?;
            local(intent.call, session, "tool call")?;
            local(intent.registration, session, "tool registration")?;
            local(intent.reservation, session, "tool reservation")?;
            if let ApprovalSource::User(command) = intent.approval {
                local(command, session, "tool approval command")?;
            }
            match &intent.footprint {
                WorkspaceFootprint::Scope(scope) => local(*scope, session, "tool scope")?,
                WorkspaceFootprint::Exact(_) | WorkspaceFootprint::None => {}
            }
            reconciliation(&intent.reconciliation, session)?;
            if let Some(process) = &intent.process {
                local(process.job(), session, "process job")?;
            }
        }
        FactData::ToolFinished { result } => {
            local(result.id, session, "tool result")?;
            local(result.call, session, "tool call")?;
            optional(result.attempt, session, "tool attempt")?;
            workspace_effect(&result.workspace, session)?;
        }
        FactData::DetachedJobOpened {
            job,
            attempt,
            process,
            ..
        } => {
            local(*job, session, "detached job")?;
            local(*attempt, session, "detached tool attempt")?;
            local(process.job(), session, "process job")?;
        }
        FactData::EffectObserved {
            observation,
            observing_result,
        } => {
            local(observation.target, session, "observed tool attempt")?;
            match &observation.basis {
                ReconciliationEvidence::Process { job, record } => {
                    local(*job, session, "reconciliation job")?;
                    local(record.job(), session, "reconciliation process job")?;
                }
                ReconciliationEvidence::External { observer, rule, .. } => {
                    local(*observer, session, "reconciliation observer")?;
                    local(*rule, session, "reconciliation rule")?;
                }
            }
            local(observation.evidence, session, "reconciliation evidence")?;
            optional(observation.workspace, session, "reconciliation observation")?;
            local(*observing_result, session, "observing tool result")?;
        }
        FactData::DetachedJobClosed { job, evidence, .. } => {
            local(*job, session, "detached job")?;
            optional(*evidence, session, "detached evidence")?;
        }
        FactData::RetryScheduled { retry } => {
            local(retry.id, session, "retry")?;
            local(retry.operation, session, "retry operation")?;
            local(retry.reservation, session, "retry reservation")?;
        }
        FactData::RetryFinished { retry, .. } => local(*retry, session, "retry")?,
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
        FactData::MeterSettled { meter, owner, .. } => {
            local(*meter, session, "meter")?;
            match owner {
                MeterOwner::Provider(id) => local(*id, session, "meter provider")?,
                MeterOwner::Tool(id) => local(*id, session, "meter tool")?,
                MeterOwner::Retry(id) => local(*id, session, "meter retry")?,
            }
        }
    }
    Ok(())
}

fn plan_revision(plan: &PlanRevision, session: SessionId) -> Result<()> {
    local(plan.id, session, "Plan")?;
    local(plan.work, session, "Plan Work")?;
    optional(plan.parent, session, "parent Plan")?;
    match plan.source {
        PlanSource::Generated { provider } => local(provider, session, "Plan provider")?,
        PlanSource::Supplied { input } => local(input, session, "Plan input")?,
        PlanSource::Imported { .. } => {}
    }
    for step in plan.steps.iter() {
        local(step.id, session, "Plan step")?;
        many(
            step.dependencies.iter().copied(),
            session,
            "Plan dependency",
        )?;
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
    optional(work.predecessor, session, "predecessor Work")?;
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
        WorkStatus::Active(_) | WorkStatus::Paused { .. } | WorkStatus::Abandoned { .. } => {}
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
        workspace_scope(&policy.scope, session)?;
        many(policy.checks.iter().copied(), session, "verification check")?;
        many(
            policy.coverage_rules.iter().copied(),
            session,
            "coverage rule",
        )?;
        for artifact in policy.artifacts.iter() {
            local(artifact.rule, session, "artifact rule")?;
        }
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

fn reconciliation(value: &ReconciliationContract, session: SessionId) -> Result<()> {
    match value {
        ReconciliationContract::Process { job } => local(*job, session, "reconciliation job"),
        ReconciliationContract::External { observer, rule, .. } => {
            local(*observer, session, "reconciliation observer")?;
            local(*rule, session, "reconciliation rule")
        }
        ReconciliationContract::None | ReconciliationContract::Unobservable => Ok(()),
    }
}

fn workspace_effect(value: &WorkspaceEffect, session: SessionId) -> Result<()> {
    if let WorkspaceEffect::Known { artifact_rules, .. } = value {
        many(
            artifact_rules.iter().copied(),
            session,
            "workspace artifact rule",
        )?;
    }
    Ok(())
}

fn context_source(source: &ContextSource, session: SessionId) -> Result<()> {
    match source {
        ContextSource::Input(id) | ContextSource::WorkPolicy(id) => {
            local(*id, session, "context input")
        }
        ContextSource::Provider(id) | ContextSource::Receipt(id) | ContextSource::Summary(id) => {
            local(*id, session, "context provider")
        }
        ContextSource::Tool(id) => local(*id, session, "context tool"),
        ContextSource::Plan(id) => local(*id, session, "context Plan"),
        ContextSource::Work(id) => local(*id, session, "context Work"),
        ContextSource::Directive(id) => local(*id, session, "context directive"),
        ContextSource::Evidence(id) => local(*id, session, "context evidence"),
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
