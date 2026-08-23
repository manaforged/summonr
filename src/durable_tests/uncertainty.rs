use super::{fixture, work};
use crate::durable::bound::Items;
use crate::durable::context::{
    ContextEntry, ContextEpoch, ContextRole, ContextSource, InputOrigin, InputRoute,
    OperationClose, OperationOutcome, UserPayload,
};
use crate::durable::effect::{
    ApprovalSource, Exposure, MeterKind, MeterSettlement, PolicyDecision, ProviderIntent,
    ProviderMessage, ProviderOutcome, ProviderPurpose, ProviderToolCall, ReconciliationContract,
    ReplayPolicy, StopReason, ToolCall, ToolIntent, ToolOutcome, ToolResult, Usage,
    WorkspaceFootprint,
};
use crate::durable::fact::{Fact, FactData, MeterOwner, OperationStageOutcome};
use crate::durable::id::{Digest, ProviderResultId};
use crate::durable::journal::{JournalWriter, inspect};
use crate::durable::work::{
    Candidate, PlanRevision, VerificationPolicy, WorkProjection, WorkStage, WorkStatus,
};
use crate::durable::workspace::{WorkspaceEffect, WorkspaceObservation};

#[test]
fn uncertain_external_effect_survives_close_and_blocks_candidate_capture() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    let plan = work::plan_revision();
    work::set_plan(&mut writer, &plan);
    work::finish_materialization(&mut writer, &plan);
    let base = work::observation(506);
    let policy = work::policy();
    work::enter_execute(&mut writer, &plan, &base, &policy);

    open_operation(&mut writer, 50, 800, 801, 802);
    start_provider(&mut writer, 51, 810, 811, 801, 802);
    let contract = ReconciliationContract::External {
        observer: fixture::id(820),
        rule: fixture::id(821),
        subject: fixture::digest("external-subject"),
    };
    plan_tool(&mut writer, 52, 810, 811, 830, 831, 832, 833, &contract);
    start_tool(&mut writer, 53, 830, 833, 834, 835, &contract);
    finish_uncertain(&mut writer, 54, 830, 832, 834, 835, 836);
    close_operation(&mut writer, 55, 801, Some(fixture::id(811)));

    assert!(
        writer
            .state()
            .uncertain_effects
            .get(&fixture::id(834))
            .is_some_and(|effect| {
                effect.work == Some(plan.work())
                    && effect.contract_digest == contract.digest().expect("contract must encode")
            })
    );
    assert_eq!(
        inspect(&path).expect("uncertain effect prefix must replay"),
        *writer.state()
    );

    open_operation(&mut writer, 56, 840, 841, 842);
    let capture = work::observation(850);
    let candidate = work::candidate(&plan, &policy, &base, &capture);
    let state = writer.state().clone();
    let bytes = fs::read(&path).expect("journal bytes must be readable");
    writer
        .append(
            fixture::id(57),
            None,
            candidate_facts(&plan, &policy, capture, &candidate, 841),
        )
        .expect_err("unresolved external effect must block candidate capture");
    assert_eq!(writer.state(), &state);
    assert_eq!(
        fs::read(&path).expect("journal bytes must be readable"),
        bytes
    );
}

fn open_operation(
    writer: &mut JournalWriter,
    commit: u128,
    input: u128,
    operation: u128,
    context: u128,
) {
    writer
        .append(
            fixture::id(commit),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(context + 100),
                    data: FactData::InputAccepted {
                        input: fixture::id(input),
                        origin: InputOrigin::WorkPolicy,
                        route: InputRoute::Start,
                        payload: UserPayload {
                            text: fixture::text("Continue Execute."),
                            images: Items::default(),
                        },
                        reserved_projection_bytes: 128,
                    },
                },
                Fact {
                    id: fixture::fact_id(context + 101),
                    data: FactData::InputApplied {
                        input: fixture::id(input),
                        operation: Some(fixture::id(operation)),
                    },
                },
                Fact {
                    id: fixture::fact_id(context + 102),
                    data: FactData::OperationOpened {
                        operation: fixture::id(operation),
                        input: fixture::id(input),
                        work: Some(fixture::id(500)),
                        stage: Some(WorkStage::Execute),
                    },
                },
                Fact {
                    id: fixture::fact_id(context + 103),
                    data: FactData::ContextEntryAdded {
                        entry: entry(
                            context,
                            ContextRole::User,
                            ContextSource::Input(fixture::id(input)),
                            "Continue Execute.",
                        ),
                    },
                },
            ],
        )
        .expect("Execute operation must open");
}

fn start_provider(
    writer: &mut JournalWriter,
    commit: u128,
    attempt: u128,
    result: u128,
    operation: u128,
    through: u128,
) {
    let request = br#"{"model":"test-model","messages":[]}"#;
    writer
        .append(
            fixture::id(commit),
            None,
            vec![Fact {
                id: fixture::fact_id(attempt + 100),
                data: FactData::ProviderStarted {
                    intent: ProviderIntent {
                        attempt: fixture::id(attempt),
                        result: fixture::id(result),
                        operation: fixture::id(operation),
                        epoch: fixture::id(601),
                        purpose: ProviderPurpose::AgentRound,
                        work: Some(fixture::id(500)),
                        plan: Some(fixture::id(501)),
                        stage: Some(WorkStage::Execute),
                        candidate: None,
                        adapter: fixture::text("openai-compatible"),
                        endpoint: fixture::text("https://example.test/v1"),
                        model: fixture::text("test-model"),
                        resource_version: None,
                        through: Some(fixture::id(through)),
                        request_bytes: u32::try_from(request.len())
                            .expect("fixture request length must fit"),
                        request_digest: Digest::hash(b"mage:provider-request:v1", request),
                        request_format: 1,
                        deadline_unix_millis: 500,
                        exposure: Exposure {
                            max_input_tokens: 100,
                            max_output_tokens: 100,
                            max_live_millis: 1_000,
                        },
                    },
                },
            }],
        )
        .expect("Execute provider intent must append");
}

#[allow(
    clippy::too_many_arguments,
    reason = "test helper fixes each provider and tool identity explicitly"
)]
fn plan_tool(
    writer: &mut JournalWriter,
    commit: u128,
    attempt: u128,
    result: u128,
    call: u128,
    batch: u128,
    tool_result: u128,
    registration: u128,
    contract: &ReconciliationContract,
) {
    writer
        .append(
            fixture::id(commit),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(call + 100),
                    data: FactData::ProviderFinished {
                        attempt: fixture::id(attempt),
                        result: fixture::id(result),
                        outcome: ProviderOutcome::Completed {
                            message: ProviderMessage {
                                text: fixture::text(""),
                                reasoning: fixture::text("perform external effect"),
                                tool_calls: Items::new(vec![ProviderToolCall {
                                    provider_id: fixture::text("external-call"),
                                    name: fixture::text("external_tool"),
                                    arguments: fixture::text("{}"),
                                }])
                                .expect("fixture tool call must fit"),
                            },
                            stop_reason: StopReason::ToolCalls,
                            reported_usage: usage(),
                        },
                    },
                },
                Fact {
                    id: fixture::fact_id(call + 101),
                    data: FactData::MeterSettled {
                        meter: fixture::id(attempt + 1),
                        owner: MeterOwner::Provider(fixture::id(attempt)),
                        kind: MeterKind::Provider,
                        settlement: MeterSettlement::Known(usage()),
                    },
                },
                Fact {
                    id: fixture::fact_id(call + 102),
                    data: FactData::ContextEntryAdded {
                        entry: entry(
                            call + 1,
                            ContextRole::Assistant,
                            ContextSource::Provider(fixture::id(result)),
                            "assistant tool call",
                        ),
                    },
                },
                Fact {
                    id: fixture::fact_id(call + 103),
                    data: FactData::ToolCallPlanned {
                        call: ToolCall {
                            id: fixture::id(call),
                            batch: fixture::id(batch),
                            result: fixture::id(tool_result),
                            provider_result: fixture::id(result),
                            provider_id: fixture::text("external-call"),
                            position: 0,
                            registration: fixture::id(registration),
                            schema_digest: fixture::digest("external-schema"),
                            effect_contract_digest: contract
                                .digest()
                                .expect("contract must encode"),
                        },
                    },
                },
            ],
        )
        .expect("provider tool obligation must append");
}

fn start_tool(
    writer: &mut JournalWriter,
    commit: u128,
    call: u128,
    registration: u128,
    attempt: u128,
    reservation: u128,
    contract: &ReconciliationContract,
) {
    writer
        .append(
            fixture::id(commit),
            None,
            vec![Fact {
                id: fixture::fact_id(attempt + 100),
                data: FactData::ToolStarted {
                    intent: ToolIntent {
                        attempt: fixture::id(attempt),
                        call: fixture::id(call),
                        registration: fixture::id(registration),
                        normalized_arguments: fixture::text("{}"),
                        decision: PolicyDecision::Allowed,
                        approval: ApprovalSource::Policy(fixture::digest("external-policy")),
                        replay: ReplayPolicy::Never,
                        deadline_unix_millis: 600,
                        reservation: fixture::id(reservation),
                        footprint: WorkspaceFootprint::None,
                        reconciliation: contract.clone(),
                        process: None,
                    },
                },
            }],
        )
        .expect("external tool intent must append");
}

fn finish_uncertain(
    writer: &mut JournalWriter,
    commit: u128,
    call: u128,
    result: u128,
    attempt: u128,
    reservation: u128,
    context: u128,
) {
    writer
        .append(
            fixture::id(commit),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(context + 100),
                    data: FactData::ToolFinished {
                        result: ToolResult {
                            id: fixture::id(result),
                            call: fixture::id(call),
                            attempt: Some(fixture::id(attempt)),
                            outcome: ToolOutcome::Uncertain {
                                diagnostic: fixture::text("remote outcome is unknown"),
                            },
                            workspace: WorkspaceEffect::None,
                        },
                    },
                },
                Fact {
                    id: fixture::fact_id(context + 101),
                    data: FactData::MeterSettled {
                        meter: fixture::id(reservation),
                        owner: MeterOwner::Tool(fixture::id(attempt)),
                        kind: MeterKind::Tool,
                        settlement: MeterSettlement::Unknown(Exposure {
                            max_input_tokens: 0,
                            max_output_tokens: 0,
                            max_live_millis: 600,
                        }),
                    },
                },
                Fact {
                    id: fixture::fact_id(context + 102),
                    data: FactData::ContextEntryAdded {
                        entry: entry(
                            context,
                            ContextRole::Tool,
                            ContextSource::Tool(fixture::id(result)),
                            "external outcome unknown",
                        ),
                    },
                },
            ],
        )
        .expect("uncertain tool terminal must append");
}

fn close_operation(
    writer: &mut JournalWriter,
    commit: u128,
    operation: u128,
    provider: Option<ProviderResultId>,
) {
    writer
        .append(
            fixture::id(commit),
            None,
            vec![Fact {
                id: fixture::fact_id(operation + 100),
                data: FactData::OperationClosed {
                    close: OperationClose {
                        id: fixture::id(operation),
                        outcome: OperationOutcome::Completed,
                        provider,
                        work: Some(fixture::id(500)),
                        work_status: Some(WorkStatus::Active(WorkStage::Execute)),
                        plan: Some(fixture::id(501)),
                        candidate: None,
                        verdict: None,
                        usage_digest: fixture::digest("operation-usage"),
                        inputs: Items::default(),
                        diagnostic: None,
                    },
                    stage_outcome: OperationStageOutcome::Stayed,
                },
            }],
        )
        .expect("Execute operation must close");
}

fn candidate_facts(
    plan: &PlanRevision,
    policy: &VerificationPolicy,
    capture: WorkspaceObservation,
    candidate: &Candidate,
    operation: u128,
) -> Vec<Fact> {
    let epoch = ContextEpoch {
        id: fixture::id(860),
        head: fixture::head(),
        stage: Some(WorkStage::Verify),
        base: Items::default(),
    };
    let status = WorkStatus::Active(WorkStage::Verify);
    vec![
        Fact {
            id: fixture::fact_id(861),
            data: FactData::WorkspaceObserved {
                observation: capture,
            },
        },
        Fact {
            id: fixture::fact_id(862),
            data: FactData::WorkSet {
                work: Box::new(work::work_state(
                    4,
                    status.clone(),
                    Some(plan.id()),
                    Some(fixture::scope().id()),
                    Some(fixture::id(506)),
                    Some(candidate.clone()),
                    Some(policy.clone()),
                    WorkProjection::Operation(fixture::id(operation)),
                    Items::default(),
                )),
            },
        },
        Fact {
            id: fixture::fact_id(863),
            data: FactData::ContextEpochOpened { epoch },
        },
        Fact {
            id: fixture::fact_id(864),
            data: FactData::OperationClosed {
                close: OperationClose {
                    id: fixture::id(operation),
                    outcome: OperationOutcome::Completed,
                    provider: None,
                    work: Some(plan.work()),
                    work_status: Some(status),
                    plan: Some(plan.id()),
                    candidate: Some(candidate.id()),
                    verdict: None,
                    usage_digest: fixture::digest("candidate-operation-usage"),
                    inputs: Items::default(),
                    diagnostic: None,
                },
                stage_outcome: OperationStageOutcome::Advanced,
            },
        },
    ]
}

fn entry(id: u128, role: ContextRole, source: ContextSource, payload: &str) -> ContextEntry {
    let mut entry = ContextEntry {
        id: fixture::id(id),
        epoch: fixture::id(601),
        role,
        source,
        payload: fixture::text(payload),
        digest: Digest::default(),
    };
    entry.digest = entry.calculated_digest();
    entry
}

fn usage() -> Usage {
    Usage {
        input_tokens: Some(10),
        output_tokens: Some(5),
        cached_input_tokens: None,
        live_millis: Some(50),
    }
}
use std::fs;
