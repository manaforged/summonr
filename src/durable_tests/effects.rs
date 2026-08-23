use super::{fixture, reducer};
use crate::durable::bound::Items;
use crate::durable::context::{
    ContextEntry, ContextRole, ContextSource, InputOrigin, InputRoute, OperationClose,
    OperationOutcome, UserPayload,
};
use crate::durable::effect::{
    ApprovalSource, BootIdentity, DetachedClose, EffectObservation, Exposure, JobState, MeterKind,
    MeterSettlement, NativeProcessScope, PolicyDecision, ProcessContainment, ProcessExit,
    ProcessIdentity, ProcessRecord, ProcessStart, ProviderIntent, ProviderMessage, ProviderOutcome,
    ProviderPurpose, ProviderToolCall, ReconciliationContract, ReconciliationEvidence,
    ReconciliationResolution, ReplayPolicy, StopReason, ToolCall, ToolIntent, ToolOutcome,
    ToolResult, Usage, WorkspaceFootprint,
};
use crate::durable::fact::{
    CommandReceipt, CommandStatus, Fact, FactData, MeterOwner, OperationStageOutcome,
};
use crate::durable::id::{Digest, ProviderResultId};
use crate::durable::journal::{JournalWriter, inspect};
use crate::durable::workspace::{PathPlatform, WorkspaceEffect, WorkspaceObservation};

#[test]
fn detached_process_stays_visible_until_typed_terminal_observation() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    reducer::start_operation(&mut writer);
    reducer::start_provider(&mut writer);

    let job = fixture::id(1404);
    let contract = ReconciliationContract::Process { job };
    finish_provider_call(
        &mut writer,
        4,
        1400,
        1401,
        1402,
        1403,
        200,
        201,
        1411,
        1412,
        "call-process",
        &contract,
    );
    let before = observation(1406);
    writer
        .append(
            fixture::id(5),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(1413),
                    data: FactData::WorkspaceObserved {
                        observation: before,
                    },
                },
                Fact {
                    id: fixture::fact_id(1414),
                    data: FactData::ToolStarted {
                        intent: ToolIntent {
                            attempt: fixture::id(1407),
                            call: fixture::id(1400),
                            registration: fixture::id(1403),
                            normalized_arguments: fixture::text("{}"),
                            decision: PolicyDecision::Allowed,
                            approval: ApprovalSource::Policy(fixture::digest("process-policy")),
                            replay: ReplayPolicy::Never,
                            deadline_unix_millis: 100,
                            reservation: fixture::id(1408),
                            footprint: WorkspaceFootprint::Scope(fixture::scope().id()),
                            reconciliation: contract.clone(),
                            process: Some(ProcessRecord::Reserved {
                                job,
                                containment: ProcessContainment::PosixGroup,
                            }),
                        },
                    },
                },
            ],
        )
        .expect("scoped process intent must append with its before observation");

    let running = ProcessRecord::Running {
        job,
        identity: process_identity(),
    };
    writer
        .append(
            fixture::id(6),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(1415),
                    data: FactData::ToolFinished {
                        result: ToolResult {
                            id: fixture::id(1402),
                            call: fixture::id(1400),
                            attempt: Some(fixture::id(1407)),
                            outcome: ToolOutcome::Completed {
                                output: fixture::text("started"),
                                job: Some(job),
                            },
                            workspace: WorkspaceEffect::None,
                        },
                    },
                },
                Fact {
                    id: fixture::fact_id(1416),
                    data: FactData::MeterSettled {
                        meter: fixture::id(1408),
                        owner: MeterOwner::Tool(fixture::id(1407)),
                        kind: MeterKind::Tool,
                        settlement: MeterSettlement::Known(usage()),
                    },
                },
                Fact {
                    id: fixture::fact_id(1417),
                    data: FactData::WorkspaceObserved {
                        observation: observation(1409),
                    },
                },
                Fact {
                    id: fixture::fact_id(1418),
                    data: FactData::DetachedJobOpened {
                        job,
                        attempt: fixture::id(1407),
                        process: running,
                        state: JobState::Running,
                        contract_digest: contract.digest().expect("process contract must encode"),
                    },
                },
                Fact {
                    id: fixture::fact_id(1419),
                    data: FactData::ContextEntryAdded {
                        entry: entry(1410, ContextSource::Tool(fixture::id(1402)), "started"),
                    },
                },
            ],
        )
        .expect("background launch must close protocol and retain its job");
    close(&mut writer, 7, 101, Some(fixture::id(201)));
    assert!(
        writer
            .state()
            .jobs
            .get(&job)
            .is_some_and(|record| record.close.is_none())
    );
    assert_eq!(
        inspect(&path).expect("open detached job must replay"),
        *writer.state()
    );

    start_operation(&mut writer, 8, 1500, 1501, 1502);
    start_provider(&mut writer, 9, 1510, 1511, 1501, 1502);
    let observer_contract = ReconciliationContract::None;
    finish_provider_call(
        &mut writer,
        10,
        1520,
        1521,
        1522,
        1523,
        1510,
        1511,
        1524,
        1528,
        "call-observer",
        &observer_contract,
    );
    writer
        .append(
            fixture::id(11),
            None,
            vec![Fact {
                id: fixture::fact_id(1529),
                data: FactData::ToolStarted {
                    intent: ToolIntent {
                        attempt: fixture::id(1525),
                        call: fixture::id(1520),
                        registration: fixture::id(1523),
                        normalized_arguments: fixture::text("{}"),
                        decision: PolicyDecision::Allowed,
                        approval: ApprovalSource::Policy(fixture::digest("observer-policy")),
                        replay: ReplayPolicy::ReplaySafe,
                        deadline_unix_millis: 200,
                        reservation: fixture::id(1526),
                        footprint: WorkspaceFootprint::None,
                        reconciliation: observer_contract,
                        process: None,
                    },
                },
            }],
        )
        .expect("observer intent must append");

    let terminal = ProcessRecord::Terminal {
        job,
        identity: process_identity(),
        exit: ProcessExit {
            code: Some(0),
            signal: None,
        },
    };
    let reconciliation = observation(1530);
    writer
        .append(
            fixture::id(12),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(1531),
                    data: FactData::ToolFinished {
                        result: ToolResult {
                            id: fixture::id(1522),
                            call: fixture::id(1520),
                            attempt: Some(fixture::id(1525)),
                            outcome: ToolOutcome::Completed {
                                output: fixture::text("terminal"),
                                job: None,
                            },
                            workspace: WorkspaceEffect::None,
                        },
                    },
                },
                Fact {
                    id: fixture::fact_id(1532),
                    data: FactData::MeterSettled {
                        meter: fixture::id(1526),
                        owner: MeterOwner::Tool(fixture::id(1525)),
                        kind: MeterKind::Tool,
                        settlement: MeterSettlement::Known(usage()),
                    },
                },
                Fact {
                    id: fixture::fact_id(1533),
                    data: FactData::ContextEntryAdded {
                        entry: entry(1527, ContextSource::Tool(fixture::id(1522)), "terminal"),
                    },
                },
                Fact {
                    id: fixture::fact_id(1534),
                    data: FactData::WorkspaceObserved {
                        observation: reconciliation.clone(),
                    },
                },
                Fact {
                    id: fixture::fact_id(1535),
                    data: FactData::EffectObserved {
                        observation: EffectObservation {
                            target: fixture::id(1407),
                            contract_digest: contract
                                .digest()
                                .expect("process contract must encode"),
                            basis: ReconciliationEvidence::Process {
                                job,
                                record: terminal,
                            },
                            resolution: ReconciliationResolution::TerminalKnown,
                            evidence: fixture::id(1536),
                            workspace: Some(reconciliation.id()),
                        },
                        observing_result: fixture::id(1522),
                    },
                },
                Fact {
                    id: fixture::fact_id(1537),
                    data: FactData::DetachedJobClosed {
                        job,
                        outcome: DetachedClose::Terminal,
                        evidence: Some(fixture::id(1536)),
                    },
                },
            ],
        )
        .expect("typed process observation must close detached job");
    close(&mut writer, 13, 1501, Some(fixture::id(1511)));
    assert!(
        writer
            .state()
            .jobs
            .get(&job)
            .is_some_and(|record| record.close == Some(DetachedClose::Terminal))
    );
    assert_eq!(
        inspect(&path).expect("closed detached job must replay"),
        *writer.state()
    );
}

#[test]
fn uncertain_background_launch_opens_a_job_that_can_be_abandoned() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    reducer::start_operation(&mut writer);
    reducer::start_provider(&mut writer);

    let job = fixture::id(1604);
    let contract = ReconciliationContract::Process { job };
    finish_provider_call(
        &mut writer,
        4,
        1600,
        1601,
        1602,
        1603,
        200,
        201,
        1611,
        1612,
        "call-uncertain-process",
        &contract,
    );
    let process = ProcessRecord::Reserved {
        job,
        containment: ProcessContainment::PosixGroup,
    };
    writer
        .append(
            fixture::id(5),
            None,
            vec![Fact {
                id: fixture::fact_id(1613),
                data: FactData::ToolStarted {
                    intent: ToolIntent {
                        attempt: fixture::id(1607),
                        call: fixture::id(1600),
                        registration: fixture::id(1603),
                        normalized_arguments: fixture::text("{}"),
                        decision: PolicyDecision::Allowed,
                        approval: ApprovalSource::Policy(fixture::digest("process-policy")),
                        replay: ReplayPolicy::Never,
                        deadline_unix_millis: 100,
                        reservation: fixture::id(1608),
                        footprint: WorkspaceFootprint::None,
                        reconciliation: contract.clone(),
                        process: Some(process.clone()),
                    },
                },
            }],
        )
        .expect("process intent must append before launch");
    writer
        .append(
            fixture::id(6),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(1614),
                    data: FactData::ToolFinished {
                        result: ToolResult {
                            id: fixture::id(1602),
                            call: fixture::id(1600),
                            attempt: Some(fixture::id(1607)),
                            outcome: ToolOutcome::Uncertain {
                                diagnostic: fixture::text("spawn outcome is unknown"),
                            },
                            workspace: WorkspaceEffect::None,
                        },
                    },
                },
                Fact {
                    id: fixture::fact_id(1615),
                    data: FactData::MeterSettled {
                        meter: fixture::id(1608),
                        owner: MeterOwner::Tool(fixture::id(1607)),
                        kind: MeterKind::Tool,
                        settlement: MeterSettlement::Unknown(Exposure {
                            max_input_tokens: 0,
                            max_output_tokens: 0,
                            max_live_millis: 100,
                        }),
                    },
                },
                Fact {
                    id: fixture::fact_id(1616),
                    data: FactData::DetachedJobOpened {
                        job,
                        attempt: fixture::id(1607),
                        process,
                        state: JobState::Uncertain,
                        contract_digest: contract.digest().expect("process contract must encode"),
                    },
                },
                Fact {
                    id: fixture::fact_id(1617),
                    data: FactData::ContextEntryAdded {
                        entry: entry(
                            1610,
                            ContextSource::Tool(fixture::id(1602)),
                            "spawn outcome unknown",
                        ),
                    },
                },
            ],
        )
        .expect("uncertain launch must close protocol and retain its job");
    close(&mut writer, 7, 101, Some(fixture::id(201)));
    assert!(
        writer
            .state()
            .jobs
            .get(&job)
            .is_some_and(|record| record.uncertain && record.close.is_none())
    );

    let close_fact = fixture::fact_id(1618);
    writer
        .append(
            fixture::id(8),
            Some(CommandReceipt {
                id: fixture::id(1619),
                digest: fixture::digest("abandon-detached-job"),
                status: CommandStatus::Accepted,
                code: None,
                result: Some(close_fact),
            }),
            vec![Fact {
                id: close_fact,
                data: FactData::DetachedJobClosed {
                    job,
                    outcome: DetachedClose::AbandonedUnknown,
                    evidence: None,
                },
            }],
        )
        .expect("typed command must abandon unknown job tracking");
    assert!(writer.state().jobs.get(&job).is_some_and(|record| {
        record.uncertain && record.close == Some(DetachedClose::AbandonedUnknown)
    }));
    assert_eq!(
        inspect(&path).expect("abandoned job must replay"),
        *writer.state()
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "test helper fixes every provider and tool identity explicitly"
)]
fn finish_provider_call(
    writer: &mut JournalWriter,
    commit: u128,
    call: u128,
    batch: u128,
    tool_result: u128,
    registration: u128,
    attempt: u128,
    provider_result: u128,
    context: u128,
    meter: u128,
    provider_id: &str,
    contract: &ReconciliationContract,
) {
    writer
        .append(
            fixture::id(commit),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(context + 100),
                    data: FactData::ProviderFinished {
                        attempt: fixture::id(attempt),
                        result: fixture::id(provider_result),
                        outcome: ProviderOutcome::Completed {
                            message: ProviderMessage {
                                text: fixture::text(""),
                                reasoning: fixture::text("observe process"),
                                tool_calls: Items::new(vec![ProviderToolCall {
                                    provider_id: fixture::text(provider_id),
                                    name: fixture::text("run_command"),
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
                    id: fixture::fact_id(context + 101),
                    data: FactData::MeterSettled {
                        meter: fixture::id(meter),
                        owner: MeterOwner::Provider(fixture::id(attempt)),
                        kind: MeterKind::Provider,
                        settlement: MeterSettlement::Known(usage()),
                    },
                },
                Fact {
                    id: fixture::fact_id(context + 102),
                    data: FactData::ContextEntryAdded {
                        entry: entry(
                            context,
                            ContextSource::Provider(fixture::id(provider_result)),
                            "assistant tool call",
                        ),
                    },
                },
                Fact {
                    id: fixture::fact_id(context + 103),
                    data: FactData::ToolCallPlanned {
                        call: ToolCall {
                            id: fixture::id(call),
                            batch: fixture::id(batch),
                            result: fixture::id(tool_result),
                            provider_result: fixture::id(provider_result),
                            provider_id: fixture::text(provider_id),
                            position: 0,
                            registration: fixture::id(registration),
                            schema_digest: fixture::digest("command-schema"),
                            effect_contract_digest: contract
                                .digest()
                                .expect("fixture contract must encode"),
                        },
                    },
                },
            ],
        )
        .expect("provider tool-call group must append");
}

fn start_operation(
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
                    id: fixture::fact_id(context + 1_000),
                    data: FactData::InputAccepted {
                        input: fixture::id(input),
                        origin: InputOrigin::User,
                        route: InputRoute::Start,
                        payload: UserPayload {
                            text: fixture::text("observe job"),
                            images: Items::default(),
                        },
                        reserved_projection_bytes: 64,
                    },
                },
                Fact {
                    id: fixture::fact_id(context + 1_001),
                    data: FactData::InputApplied {
                        input: fixture::id(input),
                        operation: Some(fixture::id(operation)),
                    },
                },
                Fact {
                    id: fixture::fact_id(context + 1_002),
                    data: FactData::OperationOpened {
                        operation: fixture::id(operation),
                        input: fixture::id(input),
                        work: None,
                        stage: None,
                    },
                },
                Fact {
                    id: fixture::fact_id(context + 1_003),
                    data: FactData::ContextEntryAdded {
                        entry: entry(
                            context,
                            ContextSource::Input(fixture::id(input)),
                            "observe job",
                        ),
                    },
                },
            ],
        )
        .expect("observer operation must open");
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
                        epoch: fixture::epoch_id(),
                        purpose: ProviderPurpose::AgentRound,
                        work: None,
                        plan: None,
                        stage: None,
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
        .expect("observer provider intent must append");
}

fn close(
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
                        work: None,
                        work_status: None,
                        plan: None,
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
        .expect("operation must close");
}

fn process_identity() -> ProcessIdentity {
    ProcessIdentity {
        boot: BootIdentity(fixture::digest("boot")),
        leader: 4242,
        started: ProcessStart(99),
        native_scope: NativeProcessScope::PosixGroup(4242),
    }
}

fn observation(id: u128) -> WorkspaceObservation {
    WorkspaceObservation::new(
        fixture::id(id),
        fixture::scope().id(),
        PathPlatform::Unix,
        Vec::new(),
    )
    .expect("fixture observation must be valid")
}

fn entry(id: u128, source: ContextSource, text: &str) -> ContextEntry {
    let role = match &source {
        ContextSource::Provider(_) => ContextRole::Assistant,
        ContextSource::Tool(_) => ContextRole::Tool,
        ContextSource::Input(_) => ContextRole::User,
        _ => ContextRole::System,
    };
    let mut entry = ContextEntry {
        id: fixture::id(id),
        epoch: fixture::epoch_id(),
        role,
        source,
        payload: fixture::text(text),
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
