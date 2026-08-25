use super::fixture;
use crate::durable::bound::Items;
use crate::durable::context::{
    ContextEntry, ContextEpoch, ContextRole, ContextSource, InputOrigin, InputRoute,
    OperationClose, OperationOutcome, UserPayload,
};
use crate::durable::effect::{
    Exposure, MeterSettlement, ProviderIntent, ProviderMessage, ProviderOutcome, ProviderPurpose,
    Usage,
};
use crate::durable::fact::{
    CommandReceipt, CommandStatus, CompletionCheck, Fact, FactData, MaterializationOutcome,
    MeterOwner, OperationStageOutcome, PublicationFootprint,
};
use crate::durable::id::{
    CandidateId, Digest, EvidenceId, MaterializationId, ObservationId, PlanRevisionId,
    ProviderResultId, WorkspaceScopeId,
};
use crate::durable::journal::JournalWriter;
use crate::durable::work::{
    AcceptanceCheck, Candidate, EvidenceRef, ItemDisposition, PlanRevision, PlanSource, PlanStep,
    RequiredObservation, VerificationPolicy, WorkItem, WorkPolicy, WorkProjection, WorkRevision,
    WorkStage, WorkState, WorkStatus, plan_path,
};
use crate::durable::workspace::{
    ChangeManifest, PathComponent, PathKey, PathPlatform, WorkspaceObservation,
};

pub(super) fn start_plan_provider(writer: &mut JournalWriter) {
    let request = br#"{"model":"test-model","messages":[]}"#;
    writer
        .append(
            fixture::id(4),
            None,
            vec![Fact {
                id: fixture::fact_id(819),
                data: FactData::ProviderStarted {
                    intent: ProviderIntent {
                        attempt: fixture::id(819),
                        result: fixture::id(820),
                        operation: fixture::id(802),
                        epoch: fixture::id(800),
                        purpose: ProviderPurpose::AgentRound,
                        through: Some(fixture::id(803)),
                        request_bytes: u32::try_from(request.len())
                            .expect("fixture request length must fit"),
                        request_digest: Digest::hash(b"mage:provider-request:v1", request),
                        exposure: Exposure {
                            max_input_tokens: 2_000,
                            max_output_tokens: 2_000,
                        },
                    },
                },
            }],
        )
        .expect("Plan provider intent must append");
}

pub(super) fn plan_provider_terminal() -> Vec<Fact> {
    let mut assistant = context_entry(
        822,
        ContextSource::Provider(fixture::id(820)),
        "Generated Plan.",
    );
    assistant.role = ContextRole::Assistant;
    vec![
        Fact {
            id: fixture::fact_id(830),
            data: FactData::ProviderFinished {
                attempt: fixture::id(819),
                result: fixture::id(820),
                outcome: ProviderOutcome::Completed {
                    message: ProviderMessage {
                        text: fixture::text("Generated Plan."),
                        reasoning: fixture::text("The steps cover the objective."),
                        tool_calls: Items::default(),
                    },
                    reported_usage: usage(),
                },
            },
        },
        Fact {
            id: fixture::fact_id(831),
            data: FactData::MeterSettled {
                owner: MeterOwner::Provider(fixture::id(819)),
                settlement: MeterSettlement::Known(usage()),
            },
        },
        Fact {
            id: fixture::fact_id(832),
            data: FactData::ContextEntryAdded { entry: assistant },
        },
    ]
}

pub(super) fn plan_revision() -> PlanRevision {
    let mut plan = PlanRevision {
        id: fixture::id(501),
        work: fixture::id(500),
        source: PlanSource::External,
        steps: Items::new(vec![PlanStep {
            id: fixture::id(503),
            title: fixture::text("Implement"),
            checks: Items::new(vec![fixture::id(504)]).expect("fixture checks must fit"),
        }])
        .expect("fixture steps must fit"),
        checks: Items::new(vec![AcceptanceCheck {
            id: fixture::id(504),
            command: fixture::text("cargo nextest run -p mage durable"),
            required_observation: Some(RequiredObservation::StdoutContains(fixture::text(
                "All changed paths are observed.",
            ))),
        }])
        .expect("fixture checks must fit"),
        markdown: fixture::text("# Plan\n\n1. Implement.\n2. Verify.\n"),
        digest: Digest::default(),
    };
    plan.digest = plan.calculated_digest().expect("fixture Plan must encode");
    plan
}

pub(super) fn set_plan(writer: &mut JournalWriter, plan: &PlanRevision) {
    let attempt: MaterializationId = fixture::id(502);
    let footprint = publication(plan, attempt);
    writer
        .append(
            fixture::id(2),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(20),
                    data: FactData::PlanSet { plan: plan.clone() },
                },
                Fact {
                    id: fixture::fact_id(21),
                    data: FactData::PlanMaterializationPlanned {
                        attempt,
                        plan: plan.id(),
                        digest: plan.digest(),
                        footprint,
                    },
                },
                Fact {
                    id: fixture::fact_id(22),
                    data: FactData::WorkSet {
                        work: Box::new(work_state(
                            1,
                            WorkStatus::Active(WorkStage::Plan),
                            None,
                            None,
                            None,
                            None,
                            None,
                            WorkProjection::Plan(plan.id()),
                            Items::default(),
                        )),
                    },
                },
            ],
        )
        .expect("Plan group must append");
}

pub(super) fn publication(plan: &PlanRevision, attempt: MaterializationId) -> PublicationFootprint {
    let destination = plan_path(
        PathPlatform::Unix,
        fixture::session(),
        plan.work(),
        plan.id(),
    )
    .expect("fixture Plan path must be valid");
    let mut temporary = destination.components().to_vec();
    let _removed = temporary.pop().expect("Plan path must have a file name");
    temporary.push(
        PathComponent::unix(format!(".mage-{}.tmp", attempt.path_component()).into_bytes())
            .expect("temporary name must be valid"),
    );
    let temporary =
        PathKey::new(PathPlatform::Unix, temporary).expect("temporary Plan path must be valid");
    PublicationFootprint {
        ancestors: Items::default(),
        temporary,
        destination,
    }
}

pub(super) fn finish_materialization(writer: &mut JournalWriter, plan: &PlanRevision) {
    let entry = context_entry(600, ContextSource::Work(plan.work()), "Plan materialized");
    writer
        .append(
            fixture::id(3),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(23),
                    data: FactData::PlanMaterializationFinished {
                        attempt: fixture::id(502),
                        plan: plan.id(),
                        outcome: MaterializationOutcome::Matched,
                    },
                },
                Fact {
                    id: fixture::fact_id(24),
                    data: FactData::WorkSet {
                        work: Box::new(work_state(
                            2,
                            WorkStatus::Active(WorkStage::Plan),
                            Some(plan.id()),
                            None,
                            None,
                            None,
                            None,
                            WorkProjection::Context(fixture::id(600)),
                            Items::default(),
                        )),
                    },
                },
                Fact {
                    id: fixture::fact_id(25),
                    data: FactData::ContextEntryAdded { entry },
                },
            ],
        )
        .expect("materialization terminal must append");
}

pub(super) fn enter_execute(
    writer: &mut JournalWriter,
    plan: &PlanRevision,
    base: &WorkspaceObservation,
    policy: &VerificationPolicy,
) {
    let (receipt, facts) = execute_group(plan, base, policy);
    writer
        .append(fixture::id(4), Some(receipt), facts)
        .expect("Plan approval and Execute base must append");
}

pub(super) fn execute_group(
    plan: &PlanRevision,
    base: &WorkspaceObservation,
    policy: &VerificationPolicy,
) -> (CommandReceipt, Vec<Fact>) {
    let epoch = ContextEpoch {
        id: fixture::id(601),
        head: fixture::head(),
        stage: Some(WorkStage::Execute),
        base: Items::default(),
    };
    let work_fact = fixture::fact_id(27);
    (
        CommandReceipt {
            id: fixture::id(603),
            digest: fixture::digest("approve-plan"),
            status: CommandStatus::Accepted,
            code: None,
            result: Some(work_fact),
        },
        vec![
            Fact {
                id: fixture::fact_id(26),
                data: FactData::WorkspaceObserved {
                    observation: base.clone(),
                },
            },
            Fact {
                id: work_fact,
                data: FactData::WorkSet {
                    work: Box::new(work_state(
                        3,
                        WorkStatus::Active(WorkStage::Execute),
                        Some(plan.id()),
                        Some(fixture::scope().id()),
                        Some(base.id()),
                        None,
                        Some(policy.clone()),
                        WorkProjection::Epoch(epoch.id),
                        Items::default(),
                    )),
                },
            },
            Fact {
                id: fixture::fact_id(28),
                data: FactData::ContextEpochOpened { epoch },
            },
            Fact {
                id: fixture::fact_id(29),
                data: FactData::InputAccepted {
                    input: fixture::id(602),
                    origin: InputOrigin::WorkPolicy,
                    route: InputRoute::Queue,
                    payload: UserPayload {
                        text: fixture::text("Continue with Execute."),
                        images: Items::default(),
                    },
                },
            },
        ],
    )
}

pub(super) fn open_operation(
    writer: &mut JournalWriter,
    commit: u128,
    input: u128,
    operation: u128,
    entry: u128,
    stage: WorkStage,
) {
    let input_id = fixture::id(input);
    let operation_id = fixture::id(operation);
    writer
        .append(
            fixture::id(commit),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(entry + 1),
                    data: FactData::InputAccepted {
                        input: input_id,
                        origin: InputOrigin::WorkPolicy,
                        route: InputRoute::Start,
                        payload: UserPayload {
                            text: fixture::text("Continue."),
                            images: Items::default(),
                        },
                    },
                },
                Fact {
                    id: fixture::fact_id(entry + 2),
                    data: FactData::InputApplied {
                        input: input_id,
                        operation: operation_id,
                    },
                },
                Fact {
                    id: fixture::fact_id(entry + 3),
                    data: FactData::OperationOpened {
                        operation: operation_id,
                        input: input_id,
                        work: Some(fixture::id(500)),
                        stage: Some(stage),
                    },
                },
                Fact {
                    id: fixture::fact_id(entry + 4),
                    data: FactData::ContextEntryAdded {
                        entry: context_entry(entry, ContextSource::Input(input_id), "Continue."),
                    },
                },
            ],
        )
        .expect("stage operation must open");
}

pub(super) fn enter_verify(
    writer: &mut JournalWriter,
    plan: &PlanRevision,
    policy: &VerificationPolicy,
    capture: WorkspaceObservation,
    candidate: &Candidate,
) {
    writer
        .append(
            fixture::id(7),
            None,
            verify_facts(plan, policy, capture, candidate),
        )
        .expect("candidate capture must enter Verify");
}

pub(super) fn verify_facts(
    plan: &PlanRevision,
    policy: &VerificationPolicy,
    capture: WorkspaceObservation,
    candidate: &Candidate,
) -> Vec<Fact> {
    let head = fixture::head();
    let epoch = ContextEpoch {
        id: fixture::id(620),
        head,
        stage: Some(WorkStage::Verify),
        base: Items::default(),
    };
    vec![
        Fact {
            id: fixture::fact_id(34),
            data: FactData::WorkspaceObserved {
                observation: capture,
            },
        },
        Fact {
            id: fixture::fact_id(35),
            data: FactData::WorkSet {
                work: Box::new(work_state(
                    4,
                    WorkStatus::Active(WorkStage::Verify),
                    Some(plan.id()),
                    Some(fixture::scope().id()),
                    Some(fixture::id(506)),
                    Some(candidate.clone()),
                    Some(policy.clone()),
                    WorkProjection::Operation(fixture::id(611)),
                    Items::default(),
                )),
            },
        },
        Fact {
            id: fixture::fact_id(36),
            data: FactData::ContextEpochOpened { epoch },
        },
        Fact {
            id: fixture::fact_id(37),
            data: FactData::OperationClosed {
                close: close(
                    611,
                    None,
                    WorkStatus::Active(WorkStage::Verify),
                    Some(candidate.id()),
                    None,
                ),
                stage_outcome: OperationStageOutcome::Advanced,
            },
        },
    ]
}

pub(super) fn start_review(writer: &mut JournalWriter) {
    let request = br#"{"model":"review-model","messages":[]}"#;
    writer
        .append(
            fixture::id(10),
            None,
            vec![Fact {
                id: fixture::fact_id(38),
                data: FactData::ProviderStarted {
                    intent: ProviderIntent {
                        attempt: fixture::id(633),
                        result: fixture::id(634),
                        operation: fixture::id(631),
                        epoch: fixture::id(620),
                        purpose: ProviderPurpose::AgentRound,
                        through: Some(fixture::id(632)),
                        request_bytes: u32::try_from(request.len())
                            .expect("fixture request length must fit"),
                        request_digest: Digest::hash(b"mage:provider-request:v1", request),
                        exposure: Exposure {
                            max_input_tokens: 2_000,
                            max_output_tokens: 2_000,
                        },
                    },
                },
            }],
        )
        .expect("Verify review intent must append");
}

pub(super) fn complete(
    writer: &mut JournalWriter,
    plan: &PlanRevision,
    policy: &VerificationPolicy,
    candidate: &Candidate,
) -> WorkState {
    let observation = observation(640);
    let verdict = fixture::id(641);
    let evidence = EvidenceRef {
        id: verdict,
        source: fixture::fact_id(40),
        digest: fixture::digest("passing-verdict"),
        summary: fixture::text("All declared checks passed."),
    };
    let completed = work_state(
        5,
        WorkStatus::Completed {
            candidate: candidate.id(),
            verdict,
        },
        Some(plan.id()),
        Some(fixture::scope().id()),
        Some(fixture::id(506)),
        Some(candidate.clone()),
        Some(policy.clone()),
        WorkProjection::Operation(fixture::id(631)),
        Items::new(vec![evidence]).expect("fixture evidence must fit"),
    );
    let mut facts = review_terminal();
    facts.extend([
        Fact {
            id: fixture::fact_id(40),
            data: FactData::CompletionCheckRecorded {
                operation: fixture::id(631),
                check: CompletionCheck::Declared(fixture::id(504)),
                command: fixture::text("cargo nextest run -p mage durable"),
                passed: true,
                output: fixture::text("All changed paths are observed."),
            },
        },
        Fact {
            id: fixture::fact_id(41),
            data: FactData::WorkspaceObserved { observation },
        },
        Fact {
            id: fixture::fact_id(42),
            data: FactData::WorkSet {
                work: Box::new(completed.clone()),
            },
        },
        Fact {
            id: fixture::fact_id(45),
            data: FactData::OperationClosed {
                close: close(
                    631,
                    Some(fixture::id(634)),
                    completed.status().clone(),
                    Some(candidate.id()),
                    Some(verdict),
                ),
                stage_outcome: OperationStageOutcome::Terminal,
            },
        },
    ]);
    writer
        .append(fixture::id(11), None, facts)
        .expect("passing Verify close must append");
    completed
}

pub(super) fn review_terminal() -> Vec<Fact> {
    let mut assistant = context_entry(
        642,
        ContextSource::Provider(fixture::id(634)),
        "Verification passed.",
    );
    assistant.role = ContextRole::Assistant;
    vec![
        Fact {
            id: fixture::fact_id(39),
            data: FactData::ProviderFinished {
                attempt: fixture::id(633),
                result: fixture::id(634),
                outcome: ProviderOutcome::Completed {
                    message: ProviderMessage {
                        text: fixture::text("Verification passed."),
                        reasoning: fixture::text("The candidate matches the Plan."),
                        tool_calls: Items::default(),
                    },
                    reported_usage: usage(),
                },
            },
        },
        Fact {
            id: fixture::fact_id(43),
            data: FactData::MeterSettled {
                owner: MeterOwner::Provider(fixture::id(633)),
                settlement: MeterSettlement::Known(usage()),
            },
        },
        Fact {
            id: fixture::fact_id(44),
            data: FactData::ContextEntryAdded { entry: assistant },
        },
    ]
}

#[allow(
    clippy::too_many_arguments,
    reason = "test fixture spells out the complete Work revision schema"
)]
pub(super) fn work_state(
    revision: u64,
    status: WorkStatus,
    selected_plan: Option<PlanRevisionId>,
    scope: Option<WorkspaceScopeId>,
    base: Option<ObservationId>,
    candidate: Option<Candidate>,
    verification: Option<VerificationPolicy>,
    projection_owner: WorkProjection,
    evidence: Items<EvidenceRef, 2_048>,
) -> WorkState {
    let item = fixture::id(503);
    let disposition = if matches!(&status, WorkStatus::Completed { .. }) {
        ItemDisposition::Completed
    } else {
        ItemDisposition::Pending
    };
    let active_item = matches!(
        &status,
        WorkStatus::Active(WorkStage::Execute | WorkStage::Verify)
    )
    .then_some(item);
    WorkState {
        id: fixture::id(500),
        revision: WorkRevision::new(revision).expect("fixture revision must be valid"),
        objective: fixture::text("Implement durable execution"),
        status,
        policy: WorkPolicy::Steps,
        selected_plan,
        items: Items::new(vec![WorkItem {
            id: item,
            disposition,
            title: fixture::text("Implement"),
            evidence: Items::default(),
            attempts: 0,
            rounds: 0,
        }])
        .expect("fixture items must fit"),
        active_item,
        acceptance_checks: Items::new(vec![fixture::id(504)]).expect("fixture checks must fit"),
        scope,
        base,
        candidate,
        verification,
        evidence,
        projection_owner,
    }
}

pub(super) fn policy() -> VerificationPolicy {
    VerificationPolicy {
        id: fixture::id(505),
        scope: fixture::scope().id(),
        checks: Items::new(vec![fixture::id(504)]).expect("fixture checks must fit"),
        repair_limit: 2,
    }
}

pub(super) fn observation(id: u128) -> WorkspaceObservation {
    WorkspaceObservation::new(
        fixture::id(id),
        fixture::scope().id(),
        PathPlatform::Unix,
        Vec::new(),
    )
    .expect("fixture observation must be valid")
}

pub(super) fn candidate(
    plan: &PlanRevision,
    policy: &VerificationPolicy,
    base: &WorkspaceObservation,
    capture: &WorkspaceObservation,
) -> Candidate {
    let manifest = ChangeManifest::new(Vec::new()).expect("empty manifest must be valid");
    let mut candidate = Candidate {
        id: fixture::id(508),
        work: plan.work(),
        execute_revision: WorkRevision::new(3).expect("fixture revision must be valid"),
        plan: plan.id(),
        policy: policy.id(),
        scope: policy.scope(),
        base: base.id(),
        capture: capture.id(),
        manifest_digest: manifest.digest(),
        digest: Digest::default(),
    };
    candidate.digest = candidate
        .calculated_digest(base.digest(), capture.digest())
        .expect("fixture candidate must encode");
    candidate
}

pub(super) fn context_entry(id: u128, source: ContextSource, payload: &str) -> ContextEntry {
    let epoch = match id {
        612 => 601,
        800..=899 => 800,
        630.. => 620,
        _ => 11,
    };
    let mut entry = ContextEntry {
        id: fixture::id(id),
        epoch: fixture::id(epoch),
        role: ContextRole::User,
        source,
        payload: fixture::text(payload),
        digest: Digest::default(),
    };
    entry.digest = entry.calculated_digest();
    entry
}

pub(super) fn close(
    operation: u128,
    provider: Option<ProviderResultId>,
    status: WorkStatus,
    candidate: Option<CandidateId>,
    verdict: Option<EvidenceId>,
) -> OperationClose {
    OperationClose {
        id: fixture::id(operation),
        outcome: OperationOutcome::Completed,
        provider,
        work: Some(fixture::id(500)),
        work_status: Some(status),
        plan: Some(fixture::id(501)),
        candidate,
        verdict,
    }
}

pub(super) fn usage() -> Usage {
    Usage {
        input_tokens: Some(100),
        output_tokens: Some(20),
        cached_input_tokens: Some(0),
        live_millis: Some(500),
    }
}
