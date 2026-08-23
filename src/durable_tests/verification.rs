use super::{fixture, work};
use crate::durable::bound::Items;
use crate::durable::context::{
    ContextEpoch, ContextRole, ContextSource, InputOrigin, InputRoute, OperationClose,
    OperationOutcome, UserPayload,
};
use crate::durable::effect::{
    ApprovalSource, Exposure, MeterKind, MeterSettlement, PolicyDecision, ProviderIntent,
    ProviderMessage, ProviderOutcome, ProviderPurpose, ProviderToolCall, ReconciliationContract,
    ReplayPolicy, StopReason, ToolCall, ToolIntent, ToolOutcome, ToolResult, WorkspaceFootprint,
};
use crate::durable::fact::{Fact, FactData, MeterOwner, OperationStageOutcome};
use crate::durable::id::Digest;
use crate::durable::journal::{JournalWriter, inspect};
use crate::durable::work::{
    Candidate, EvidenceRef, PlanRevision, VerificationPolicy, WorkProjection, WorkStage, WorkStatus,
};
use crate::durable::workspace::WorkspaceEffect;

#[test]
fn failed_verify_check_returns_to_execute_as_one_durable_group() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    let plan = work::plan_revision();
    work::set_plan(&mut writer, &plan);
    work::finish_materialization(&mut writer, &plan);
    let base = work::observation(506);
    let policy = work::policy();
    work::enter_execute(&mut writer, &plan, &base, &policy);
    work::open_operation(&mut writer, 6, 610, 611, 612, WorkStage::Execute);
    let capture = work::observation(507);
    let candidate = work::candidate(&plan, &policy, &base, &capture);
    work::enter_verify(&mut writer, &plan, &policy, capture, &candidate);
    work::open_operation(&mut writer, 9, 630, 631, 632, WorkStage::Verify);
    start_provider(&mut writer, &plan, &policy, &candidate);
    plan_check(&mut writer);
    start_check(&mut writer);

    let facts = failed_group(&plan, &policy);
    let incomplete: Vec<_> = facts
        .iter()
        .filter(|fact| {
            !matches!(
                fact.data,
                FactData::ContextEpochOpened { .. } | FactData::InputAccepted { .. }
            )
        })
        .cloned()
        .collect();
    writer
        .append(fixture::id(13), None, incomplete)
        .expect_err("Verify return cannot omit its Execute epoch and continuation");
    writer
        .append(fixture::id(13), None, facts)
        .expect("failed Verify check must return to Execute atomically");

    let current = writer
        .state()
        .works
        .get(&plan.work())
        .expect("Work must remain durable");
    assert_eq!(current.status().stage(), Some(WorkStage::Execute));
    assert!(current.candidate().is_none());
    assert_eq!(writer.state().current_epoch, Some(fixture::id(721)));
    assert_eq!(
        inspect(&path).expect("Verify return must replay"),
        *writer.state()
    );
}

fn start_provider(
    writer: &mut JournalWriter,
    plan: &PlanRevision,
    policy: &VerificationPolicy,
    candidate: &Candidate,
) {
    let request = br#"{"model":"review-model","messages":[]}"#;
    writer
        .append(
            fixture::id(10),
            None,
            vec![Fact {
                id: fixture::fact_id(700),
                data: FactData::ProviderStarted {
                    intent: ProviderIntent {
                        attempt: fixture::id(700),
                        result: fixture::id(701),
                        operation: fixture::id(631),
                        epoch: fixture::id(620),
                        purpose: ProviderPurpose::AgentRound,
                        work: Some(plan.work()),
                        plan: Some(plan.id()),
                        stage: Some(WorkStage::Verify),
                        candidate: Some(candidate.id()),
                        adapter: fixture::text("openai-compatible"),
                        endpoint: policy.reviewer.endpoint.clone(),
                        model: policy.reviewer.model.clone(),
                        resource_version: None,
                        through: Some(fixture::id(632)),
                        request_bytes: u32::try_from(request.len())
                            .expect("fixture request length must fit"),
                        request_digest: Digest::hash(b"mage:provider-request:v1", request),
                        request_format: policy.reviewer.request_format,
                        deadline_unix_millis: 1_800_000_000_000,
                        exposure: Exposure {
                            max_input_tokens: 2_000,
                            max_output_tokens: 2_000,
                            max_live_millis: 30_000,
                        },
                    },
                },
            }],
        )
        .expect("Verify provider must start");
}

fn plan_check(writer: &mut JournalWriter) {
    let contract = ReconciliationContract::None;
    let mut assistant = work::context_entry(
        704,
        ContextSource::Provider(fixture::id(701)),
        "Run the declared check.",
    );
    assistant.role = ContextRole::Assistant;
    writer
        .append(
            fixture::id(11),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(702),
                    data: FactData::ProviderFinished {
                        attempt: fixture::id(700),
                        result: fixture::id(701),
                        outcome: ProviderOutcome::Completed {
                            message: ProviderMessage {
                                text: fixture::text(""),
                                reasoning: fixture::text("Run the acceptance check."),
                                tool_calls: Items::new(vec![ProviderToolCall {
                                    provider_id: fixture::text("verify-check"),
                                    name: fixture::text("run_command"),
                                    arguments: fixture::text(r#"{"command":"cargo nextest run"}"#),
                                }])
                                .expect("fixture tool call must fit"),
                            },
                            stop_reason: StopReason::ToolCalls,
                            reported_usage: work::usage(),
                        },
                    },
                },
                Fact {
                    id: fixture::fact_id(703),
                    data: FactData::MeterSettled {
                        meter: fixture::id(702),
                        owner: MeterOwner::Provider(fixture::id(700)),
                        kind: MeterKind::Provider,
                        settlement: MeterSettlement::Known(work::usage()),
                    },
                },
                Fact {
                    id: fixture::fact_id(704),
                    data: FactData::ContextEntryAdded { entry: assistant },
                },
                Fact {
                    id: fixture::fact_id(705),
                    data: FactData::ToolCallPlanned {
                        call: ToolCall {
                            id: fixture::id(710),
                            batch: fixture::id(711),
                            result: fixture::id(712),
                            provider_result: fixture::id(701),
                            provider_id: fixture::text("verify-check"),
                            position: 0,
                            registration: fixture::id(713),
                            schema_digest: fixture::digest("verify-command-schema"),
                            effect_contract_digest: contract
                                .digest()
                                .expect("fixture contract must encode"),
                        },
                    },
                },
            ],
        )
        .expect("Verify provider must publish its check obligation");
}

fn start_check(writer: &mut JournalWriter) {
    writer
        .append(
            fixture::id(12),
            None,
            vec![Fact {
                id: fixture::fact_id(706),
                data: FactData::ToolStarted {
                    intent: ToolIntent {
                        attempt: fixture::id(714),
                        call: fixture::id(710),
                        registration: fixture::id(713),
                        normalized_arguments: fixture::text(r#"{"command":"cargo nextest run"}"#),
                        decision: PolicyDecision::Allowed,
                        approval: ApprovalSource::Policy(fixture::digest("verify-policy")),
                        replay: ReplayPolicy::Never,
                        deadline_unix_millis: 1_800_000_000_000,
                        reservation: fixture::id(715),
                        footprint: WorkspaceFootprint::None,
                        reconciliation: ReconciliationContract::None,
                        process: None,
                    },
                },
            }],
        )
        .expect("Verify check must start");
}

fn failed_group(plan: &PlanRevision, policy: &VerificationPolicy) -> Vec<Fact> {
    let finding = EvidenceRef {
        id: fixture::id(720),
        source: fixture::fact_id(707),
        digest: fixture::digest("failed-check"),
        summary: fixture::text("The declared check failed."),
    };
    let status = WorkStatus::Active(WorkStage::Execute);
    let epoch = ContextEpoch {
        id: fixture::id(721),
        head: fixture::head(),
        stage: Some(WorkStage::Execute),
        base: Items::default(),
    };
    vec![
        Fact {
            id: fixture::fact_id(707),
            data: FactData::ToolFinished {
                result: ToolResult {
                    id: fixture::id(712),
                    call: fixture::id(710),
                    attempt: Some(fixture::id(714)),
                    outcome: ToolOutcome::Failed {
                        diagnostic: fixture::text("acceptance check failed"),
                    },
                    workspace: WorkspaceEffect::None,
                },
            },
        },
        Fact {
            id: fixture::fact_id(708),
            data: FactData::MeterSettled {
                meter: fixture::id(715),
                owner: MeterOwner::Tool(fixture::id(714)),
                kind: MeterKind::Tool,
                settlement: MeterSettlement::Known(work::usage()),
            },
        },
        Fact {
            id: fixture::fact_id(709),
            data: FactData::ContextEntryAdded {
                entry: work::context_entry(
                    716,
                    ContextSource::Tool(fixture::id(712)),
                    "acceptance check failed",
                ),
            },
        },
        Fact {
            id: fixture::fact_id(710),
            data: FactData::WorkSet {
                work: Box::new(work::work_state(
                    5,
                    status.clone(),
                    Some(plan.id()),
                    Some(fixture::scope().id()),
                    Some(fixture::id(506)),
                    None,
                    Some(policy.clone()),
                    WorkProjection::Pending(fixture::id(711)),
                    Items::new(vec![finding]).expect("fixture evidence must fit"),
                )),
            },
        },
        Fact {
            id: fixture::fact_id(711),
            data: FactData::OperationClosed {
                close: OperationClose {
                    id: fixture::id(631),
                    outcome: OperationOutcome::Completed,
                    provider: Some(fixture::id(701)),
                    work: Some(plan.work()),
                    work_status: Some(status),
                    plan: Some(plan.id()),
                    candidate: None,
                    verdict: Some(fixture::id(720)),
                    usage_digest: fixture::digest("failed-verify-usage"),
                    inputs: Items::default(),
                    diagnostic: None,
                },
                stage_outcome: OperationStageOutcome::Returned,
            },
        },
        Fact {
            id: fixture::fact_id(712),
            data: FactData::ContextEpochOpened { epoch },
        },
        Fact {
            id: fixture::fact_id(713),
            data: FactData::InputAccepted {
                input: fixture::id(722),
                origin: InputOrigin::WorkPolicy,
                route: InputRoute::Queue,
                payload: UserPayload {
                    text: fixture::text("Repair the failed check."),
                    images: Items::default(),
                },
                reserved_projection_bytes: 128,
            },
        },
    ]
}
