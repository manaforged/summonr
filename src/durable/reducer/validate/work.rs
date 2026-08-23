use super::effect::work_blocked;
use super::{find_observation, known_scope, unknown};
use crate::durable::encoding::Commit;
use crate::durable::fact::{FactData, MaterializationOutcome};
use crate::durable::id::{ObservationId, PlanRevisionId, WorkId};
use crate::durable::reducer::{ProviderState, ReduceError, Result, SessionState};
use crate::durable::work::{
    ItemDisposition, PlanRevision, PlanSource, VerificationPolicy, WorkError, WorkStage, WorkState,
    WorkStatus, plan_path as build_plan_path,
};
use crate::durable::workspace::PathKey;

pub(super) fn validate_plan(
    state: &SessionState,
    commit: &Commit,
    plan: &PlanRevision,
) -> Result<()> {
    plan.validate()?;
    match plan.source {
        PlanSource::Generated { provider } => {
            let known = state.providers.values().any(|attempt| match attempt {
                ProviderState::Started(intent) | ProviderState::Finished { intent, .. } => {
                    intent.result == provider
                }
            }) || commit.facts.iter().any(|fact| {
                matches!(&fact.data, FactData::ProviderFinished { result, .. } if *result == provider)
            });
            if !known {
                return Err(unknown("Plan provider result"));
            }
        }
        PlanSource::Supplied { input } => {
            let known = state.inputs.contains_key(&input)
                || commit.facts.iter().any(|fact| {
                    matches!(&fact.data, FactData::InputAccepted { input: id, .. } if *id == input)
                });
            if !known {
                return Err(unknown("Plan input"));
            }
        }
        PlanSource::Imported { .. } => {}
    }
    if state.plans.contains_key(&plan.id()) {
        return Err(ReduceError::Duplicate {
            kind: "Plan revision",
        });
    }
    if let Some(parent) = plan.parent() {
        let Some(parent_plan) = find_plan(state, commit, parent) else {
            return Err(unknown("parent Plan"));
        };
        if parent_plan.work() != plan.work() {
            return Err(ReduceError::State("Plan parent belongs to another Work"));
        }
    }
    Ok(())
}

pub(super) fn validate_work(state: &SessionState, commit: &Commit, work: &WorkState) -> Result<()> {
    work.validate()?;
    if work.evidence.iter().any(|evidence| {
        !state.facts.contains_key(&evidence.source)
            && !commit.facts.iter().any(|fact| fact.id == evidence.source)
    }) {
        return Err(unknown("Work evidence source"));
    }
    if let Some(current) = state.works.get(&work.id()) {
        if current.status().terminal() {
            return Err(ReduceError::State("terminal Work is immutable"));
        }
        if work.revision().get() != current.revision().get().saturating_add(1) {
            return Err(ReduceError::State("Work revision is not monotonic"));
        }
        if !legal_transition(current.status(), work.status()) {
            return Err(ReduceError::State(
                "Work status or stage transition is illegal",
            ));
        }
        validate_revision(current, work, commit)?;
        if current.base().is_some() && work.base() != current.base() {
            return Err(ReduceError::State("Work Execute base is immutable"));
        }
        if current.base().is_some() && !capture_shape(current.verification(), work.verification()) {
            return Err(ReduceError::State(
                "workspace scope and artifact exclusions cannot change after base capture",
            ));
        }
        if let Some(open) = &state.operation
            && open.work == Some(work.id())
            && work.status().stage() != open.stage
            && !commit.facts.iter().any(|fact| {
                matches!(&fact.data, FactData::OperationClosed { close, .. } if close.id == open.id)
            })
        {
            return Err(ReduceError::State(
                "Work cannot change stage inside an open operation before its close",
            ));
        }
    } else {
        creation(state, commit, work)?;
    }
    if let Some(plan) = work.selected_plan() {
        let revision = find_plan(state, commit, plan).ok_or_else(|| unknown("selected Plan"))?;
        if revision.work() != work.id() {
            return Err(ReduceError::State("selected Plan belongs to another Work"));
        }
        let checks: Vec<_> = revision.checks.iter().map(|check| check.id).collect();
        if work.acceptance_checks.as_slice() != checks
            || work
                .verification()
                .is_some_and(|policy| policy.checks.as_slice() != checks)
        {
            return Err(ReduceError::State(
                "Work and verification checks do not match the selected Plan",
            ));
        }
    }
    if let Some(scope) = work.scope()
        && !known_scope(state, commit, scope)
    {
        return Err(unknown("Work workspace scope"));
    }
    if let Some(base) = work.base()
        && let Some(scope) = work.scope()
    {
        let observation = find_observation(state, commit, base)
            .ok_or_else(|| unknown("Work base observation"))?;
        if observation.scope() != scope {
            return Err(ReduceError::State(
                "Work base observation does not match its workspace scope",
            ));
        }
    }
    if let Some(candidate) = work.candidate() {
        let base = find_observation(state, commit, candidate.base)
            .ok_or_else(|| unknown("candidate base observation"))?;
        let capture = find_observation(state, commit, candidate.capture)
            .ok_or_else(|| unknown("candidate capture observation"))?;
        if base.scope() != candidate.scope || capture.scope() != candidate.scope {
            return Err(WorkError::CandidateBinding.into());
        }
        if base.changes(capture)?.digest() != candidate.manifest_digest {
            return Err(WorkError::CandidateManifest.into());
        }
        if candidate.digest != candidate.calculated_digest(base.digest(), capture.digest())? {
            return Err(WorkError::CandidateDigest.into());
        }
        if matches!(work.status(), WorkStatus::Completed { .. }) {
            let recheck = commit.facts.iter().find_map(|fact| match &fact.data {
                FactData::WorkspaceObserved { observation }
                    if observation.scope() == candidate.scope =>
                {
                    Some(observation)
                }
                _ => None,
            });
            if recheck.is_none_or(|observation| observation.digest() != capture.digest()) {
                return Err(ReduceError::State(
                    "completed Work requires a fresh unchanged candidate observation",
                ));
            }
        }
    }
    if matches!(
        work.status().stage(),
        Some(WorkStage::Execute | WorkStage::Verify)
    ) {
        let plan = work
            .selected_plan()
            .ok_or(ReduceError::State("stage has no Plan"))?;
        if !materialized(state, commit, plan) {
            return Err(ReduceError::State(
                "Execute and Verify require a usable Plan materialization",
            ));
        }
    }
    if state.works.get(&work.id()).is_some_and(|current| {
        effect_boundary(current, work) && work_blocked(state, commit, work.id())
    }) {
        return Err(ReduceError::State(
            "open job or uncertain effect blocks this Work boundary",
        ));
    }
    Ok(())
}

fn effect_boundary(current: &WorkState, next: &WorkState) -> bool {
    (current.base().is_none() && next.base().is_some())
        || (current.candidate() != next.candidate() && next.candidate().is_some())
        || matches!(next.status(), WorkStatus::Completed { .. })
}

fn creation(state: &SessionState, commit: &Commit, work: &WorkState) -> Result<()> {
    if work.revision().get() != 1
        || !matches!(work.status(), WorkStatus::Active(WorkStage::Plan))
        || work.base().is_some()
        || work.candidate().is_some()
    {
        return Err(ReduceError::State(
            "new Work must start at revision 1 in Plan without a base or candidate",
        ));
    }
    if let Some(predecessor) = work.predecessor {
        let prior =
            find_work(state, commit, predecessor).ok_or_else(|| unknown("predecessor Work"))?;
        if predecessor == work.id() || !prior.status().terminal() {
            return Err(ReduceError::State(
                "successor Work requires a different terminal predecessor",
            ));
        }
    }
    Ok(())
}

fn validate_revision(current: &WorkState, next: &WorkState, commit: &Commit) -> Result<()> {
    if current.objective != next.objective
        || current.policy != next.policy
        || current.predecessor != next.predecessor
    {
        return Err(ReduceError::State(
            "Work objective, continuation policy, and predecessor are immutable",
        ));
    }
    preserve_evidence(current, next)?;
    preserve_items(current, next)?;
    plan_change(current, next)?;
    policy_change(current, next, commit)?;
    base_capture(current, next, commit)?;
    candidate_change(current, next, commit)
}

fn preserve_evidence(current: &WorkState, next: &WorkState) -> Result<()> {
    if current.evidence.iter().any(|evidence| {
        next.evidence
            .iter()
            .find(|candidate| candidate.id == evidence.id)
            != Some(evidence)
    }) {
        return Err(ReduceError::State(
            "a later Work revision cannot remove or edit evidence",
        ));
    }
    Ok(())
}

fn preserve_items(current: &WorkState, next: &WorkState) -> Result<()> {
    for item in current.items.iter() {
        let Some(candidate) = next.items.iter().find(|candidate| candidate.id == item.id) else {
            return Err(ReduceError::State(
                "removed Work items must remain present as retired items",
            ));
        };
        if item
            .evidence
            .iter()
            .any(|evidence| !candidate.evidence.iter().any(|next| next == evidence))
        {
            return Err(ReduceError::State(
                "a later Work item cannot remove evidence",
            ));
        }
        let disposition_regressed = match item.disposition {
            ItemDisposition::Completed => !matches!(
                candidate.disposition,
                ItemDisposition::Completed | ItemDisposition::Retired
            ),
            ItemDisposition::Retired => !matches!(candidate.disposition, ItemDisposition::Retired),
            _ => false,
        };
        if disposition_regressed {
            return Err(ReduceError::State(
                "completed and retired Work items cannot regress",
            ));
        }
    }
    Ok(())
}

fn plan_change(current: &WorkState, next: &WorkState) -> Result<()> {
    if current.selected_plan() != next.selected_plan()
        && current.status().stage() != Some(WorkStage::Plan)
        && next.status().stage() != Some(WorkStage::Plan)
    {
        return Err(ReduceError::State(
            "selected Plan can change only at a Plan boundary",
        ));
    }
    Ok(())
}

fn policy_change(current: &WorkState, next: &WorkState, commit: &Commit) -> Result<()> {
    if current.base().is_none() || current.verification() == next.verification() {
        return Ok(());
    }
    let next_policy = next.verification().ok_or(ReduceError::State(
        "verification policy cannot be removed after base capture",
    ))?;
    let revised_plan = next.selected_plan().is_some_and(|selected| {
        current.selected_plan() != Some(selected)
            && commit.facts.iter().any(
                |fact| matches!(&fact.data, FactData::PlanSet { plan } if plan.id() == selected),
            )
    });
    if next.status().stage() != Some(WorkStage::Plan)
        || current.verification().map(VerificationPolicy::id) == Some(next_policy.id())
        || !revised_plan
    {
        return Err(ReduceError::State(
            "verification policy changes require Plan, a new Plan revision, and a new policy ID",
        ));
    }
    Ok(())
}

fn base_capture(current: &WorkState, next: &WorkState, commit: &Commit) -> Result<()> {
    let (None, Some(base)) = (current.base(), next.base()) else {
        return Ok(());
    };
    if current.status().stage() != Some(WorkStage::Plan)
        || next.status().stage() != Some(WorkStage::Execute)
        || !same_observation(commit, base)
    {
        return Err(ReduceError::State(
            "first Execute transition requires its base observation in the same commit",
        ));
    }
    Ok(())
}

fn candidate_change(current: &WorkState, next: &WorkState, commit: &Commit) -> Result<()> {
    if current.candidate() == next.candidate() {
        return Ok(());
    }
    let Some(candidate) = next.candidate() else {
        if current.status().stage() == Some(WorkStage::Verify)
            && matches!(
                next.status().stage(),
                Some(WorkStage::Execute | WorkStage::Plan)
            )
        {
            return Ok(());
        }
        return Err(ReduceError::State(
            "candidate can be cleared only when Verify returns to Execute or Plan",
        ));
    };
    if current.status().stage() != Some(WorkStage::Execute)
        || next.status().stage() != Some(WorkStage::Verify)
        || candidate.execute_revision != current.revision()
        || !same_observation(commit, candidate.capture)
    {
        return Err(ReduceError::State(
            "candidate capture must bind the last Execute revision and a same-commit observation",
        ));
    }
    Ok(())
}

fn same_observation(commit: &Commit, id: ObservationId) -> bool {
    commit.facts.iter().any(|fact| {
        matches!(&fact.data, FactData::WorkspaceObserved { observation } if observation.id() == id)
    })
}

fn capture_shape(current: Option<&VerificationPolicy>, next: Option<&VerificationPolicy>) -> bool {
    match (current, next) {
        (Some(current), Some(next)) => current.same_capture(next),
        (None, None) => true,
        _ => false,
    }
}

fn legal_transition(current: &WorkStatus, next: &WorkStatus) -> bool {
    match (current, next) {
        (WorkStatus::Active(left), WorkStatus::Active(right)) => matches!(
            (left, right),
            (WorkStage::Plan, WorkStage::Plan | WorkStage::Execute)
                | (
                    WorkStage::Execute,
                    WorkStage::Execute | WorkStage::Plan | WorkStage::Verify
                )
                | (
                    WorkStage::Verify,
                    WorkStage::Verify | WorkStage::Execute | WorkStage::Plan
                )
        ),
        (WorkStatus::Active(stage), WorkStatus::Paused { stage: next, .. })
        | (WorkStatus::Paused { stage, .. }, WorkStatus::Active(next))
        | (WorkStatus::Paused { stage, .. }, WorkStatus::Paused { stage: next, .. }) => {
            stage == next
        }
        (WorkStatus::Active(WorkStage::Verify), WorkStatus::Completed { .. })
        | (WorkStatus::Active(_), WorkStatus::Abandoned { .. })
        | (WorkStatus::Paused { .. }, WorkStatus::Abandoned { .. }) => true,
        (
            WorkStatus::Completed { .. }
            | WorkStatus::Abandoned { .. }
            | WorkStatus::LegacyTerminal { .. },
            _,
        )
        | (_, WorkStatus::LegacyTerminal { .. }) => false,
        _ => false,
    }
}

pub(super) fn materialized(state: &SessionState, commit: &Commit, plan: PlanRevisionId) -> bool {
    state.materializations.values().any(|item| {
        item.plan == plan
            && item
                .outcome
                .as_ref()
                .is_some_and(MaterializationOutcome::usable)
    }) || commit.facts.iter().any(|fact| {
        matches!(&fact.data, FactData::PlanMaterializationFinished { plan: id, outcome, .. } if *id == plan && outcome.usable())
    })
}

pub(super) fn find_plan<'a>(
    state: &'a SessionState,
    commit: &'a Commit,
    id: PlanRevisionId,
) -> Option<&'a PlanRevision> {
    state.plans.get(&id).or_else(|| {
        commit.facts.iter().find_map(|fact| match &fact.data {
            FactData::PlanSet { plan } if plan.id() == id => Some(plan),
            _ => None,
        })
    })
}

pub(super) fn find_work<'a>(
    state: &'a SessionState,
    commit: &'a Commit,
    id: WorkId,
) -> Option<&'a WorkState> {
    commit
        .facts
        .iter()
        .rev()
        .find_map(|fact| match &fact.data {
            FactData::WorkSet { work } if work.id() == id => Some(work.as_ref()),
            _ => None,
        })
        .or_else(|| state.works.get(&id))
}

pub(super) fn plan_path(state: &SessionState, plan: &PlanRevision) -> Result<PathKey> {
    let scope = state
        .default_scope
        .and_then(|id| state.scopes.get(&id))
        .ok_or_else(|| unknown("default workspace scope"))?;
    build_plan_path(
        scope.platform(),
        plan.id().session(),
        plan.work(),
        plan.id(),
    )
    .map_err(Into::into)
}
