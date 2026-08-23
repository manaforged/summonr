use std::collections::HashSet;

use super::{ReduceError, Result};
use crate::durable::encoding::Commit;
use crate::durable::fact::FactData;
use crate::durable::id::ScopedId;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
enum Entity {
    ContextEntry,
    Directive,
    EffectObservation,
    Epoch,
    Fork,
    Input,
    InputDisposition,
    InputRoute,
    Job,
    Materialization,
    Meter,
    Observation,
    Operation,
    Plan,
    Provider,
    Retry,
    ToolAttempt,
    ToolCall,
    ToolResult,
    Work,
    WorkspaceScope,
}

impl Entity {
    fn label(self) -> &'static str {
        match self {
            Self::ContextEntry => "context entry",
            Self::Directive => "directive mutation",
            Self::EffectObservation => "effect observation target",
            Self::Epoch => "context epoch",
            Self::Fork => "fork mutation",
            Self::Input => "accepted input",
            Self::InputDisposition => "input disposition",
            Self::InputRoute => "input route mutation",
            Self::Job => "detached job mutation",
            Self::Materialization => "Plan materialization mutation",
            Self::Meter => "meter settlement",
            Self::Observation => "workspace observation",
            Self::Operation => "operation mutation",
            Self::Plan => "Plan revision",
            Self::Provider => "provider mutation",
            Self::Retry => "retry mutation",
            Self::ToolAttempt => "tool attempt mutation",
            Self::ToolCall => "tool call",
            Self::ToolResult => "tool result",
            Self::Work => "Work revision",
            Self::WorkspaceScope => "workspace scope",
        }
    }
}

pub(super) fn validate(commit: &Commit) -> Result<()> {
    let mut seen = HashSet::new();
    for fact in &commit.facts {
        match &fact.data {
            FactData::SessionCreated { scope, .. } => {
                record(&mut seen, Entity::WorkspaceScope, scope.id)?
            }
            FactData::LegacyToolUncertain { result, .. } => {
                record(&mut seen, Entity::ToolResult, *result)?
            }
            FactData::PlanSet { plan } => record(&mut seen, Entity::Plan, plan.id)?,
            FactData::PlanMaterializationPlanned { attempt, .. }
            | FactData::PlanMaterializationFinished { attempt, .. } => {
                record(&mut seen, Entity::Materialization, *attempt)?
            }
            FactData::ForkPlanned { intent, .. }
            | FactData::ForkCancelRequested { intent }
            | FactData::ForkFinished { intent, .. }
            | FactData::ForkSnapshotSealed { intent, .. } => {
                record(&mut seen, Entity::Fork, *intent)?
            }
            FactData::InputAccepted { input, .. } => record(&mut seen, Entity::Input, *input)?,
            FactData::InputRouted { input, .. } => record(&mut seen, Entity::InputRoute, *input)?,
            FactData::InputApplied { input, .. } | FactData::InputCancelled { input, .. } => {
                record(&mut seen, Entity::InputDisposition, *input)?
            }
            FactData::DirectiveChanged { directive } => {
                record(&mut seen, Entity::Directive, directive.id)?
            }
            FactData::WorkSet { work } => record(&mut seen, Entity::Work, work.id)?,
            FactData::WorkspaceObserved { observation } => {
                record(&mut seen, Entity::Observation, observation.id)?
            }
            FactData::OperationOpened { operation, .. }
            | FactData::AbortRequested { operation, .. } => {
                record(&mut seen, Entity::Operation, *operation)?
            }
            FactData::OperationClosed { close, .. } => {
                record(&mut seen, Entity::Operation, close.id)?
            }
            FactData::ProviderStarted { intent } => {
                record(&mut seen, Entity::Provider, intent.attempt)?
            }
            FactData::ProviderFinished { attempt, .. } => {
                record(&mut seen, Entity::Provider, *attempt)?
            }
            FactData::ToolCallPlanned { call } => {
                record(&mut seen, Entity::ToolCall, call.id)?;
                record(&mut seen, Entity::ToolResult, call.result)?;
            }
            FactData::ToolStarted { intent } => {
                record(&mut seen, Entity::ToolAttempt, intent.attempt)?
            }
            FactData::ToolFinished { result } => {
                record(&mut seen, Entity::ToolResult, result.id)?;
                if let Some(attempt) = result.attempt {
                    record(&mut seen, Entity::ToolAttempt, attempt)?;
                }
            }
            FactData::DetachedJobOpened { job, .. } | FactData::DetachedJobClosed { job, .. } => {
                record(&mut seen, Entity::Job, *job)?
            }
            FactData::EffectObserved { observation, .. } => {
                record(&mut seen, Entity::EffectObservation, observation.target)?;
            }
            FactData::RetryScheduled { retry } => record(&mut seen, Entity::Retry, retry.id)?,
            FactData::RetryFinished { retry, .. } => record(&mut seen, Entity::Retry, *retry)?,
            FactData::ContextEpochOpened { epoch } => record(&mut seen, Entity::Epoch, epoch.id)?,
            FactData::ContextEntryAdded { entry } => {
                record(&mut seen, Entity::ContextEntry, entry.id)?
            }
            FactData::MeterSettled { meter, .. } => record(&mut seen, Entity::Meter, *meter)?,
            FactData::LegacyImported { .. }
            | FactData::LegacySourceAccepted { .. }
            | FactData::BudgetChanged { .. }
            | FactData::ForkedFrom { .. }
            | FactData::CommandRejected { .. }
            | FactData::ConversationCleared { .. } => {}
        }
    }
    Ok(())
}

fn record<K>(
    seen: &mut HashSet<(Entity, [u8; 32])>,
    entity: Entity,
    id: ScopedId<K>,
) -> Result<()> {
    let mut key = [0; 32];
    key[..16].copy_from_slice(id.session().as_bytes());
    key[16..].copy_from_slice(id.local().as_bytes());
    if !seen.insert((entity, key)) {
        return Err(ReduceError::Duplicate {
            kind: entity.label(),
        });
    }
    Ok(())
}
