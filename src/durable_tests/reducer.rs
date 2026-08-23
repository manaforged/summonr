use super::fixture;
use crate::durable::bound::{Items, Text};
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
use crate::durable::id::Digest;
use crate::durable::journal::{JournalWriter, inspect};
use crate::durable::workspace::WorkspaceEffect;

#[test]
fn provider_and_started_tool_lifecycle_replays_at_every_prefix() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);

    start_operation(&mut writer);
    assert_eq!(
        inspect(&path).expect("start prefix must replay"),
        *writer.state()
    );

    start_provider(&mut writer);
    assert_eq!(
        inspect(&path).expect("provider intent must replay"),
        *writer.state()
    );

    finish_provider_with_tool(&mut writer);
    assert_eq!(
        inspect(&path).expect("tool batch intent must replay"),
        *writer.state()
    );

    start_tool(&mut writer);
    assert_eq!(
        inspect(&path).expect("tool intent must replay"),
        *writer.state()
    );

    finish_tool(&mut writer);
    assert_eq!(
        inspect(&path).expect("tool terminal must replay"),
        *writer.state()
    );

    close_operation(&mut writer);
    assert_eq!(
        inspect(&path).expect("closed operation must replay"),
        *writer.state()
    );
    assert!(writer.state().operation.is_none());
}

#[test]
fn provider_intent_must_match_the_frozen_epoch_head() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    start_operation(&mut writer);

    let state = writer.state().clone();
    let bytes = fs::read(&path).expect("journal bytes must be readable");
    let mut intent = provider_intent();
    intent.model = fixture::text("substituted-model");
    writer
        .append(
            fixture::id(3),
            None,
            vec![Fact {
                id: fixture::fact_id(7),
                data: FactData::ProviderStarted { intent },
            }],
        )
        .expect_err("provider model drift must be rejected before append");
    assert_eq!(writer.state(), &state);
    assert_eq!(
        fs::read(&path).expect("journal bytes must be readable"),
        bytes
    );

    start_provider(&mut writer);
}

#[test]
fn one_commit_cannot_repeat_a_semantic_identity() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    let mut facts = operation_facts();
    let mut duplicate = facts
        .last()
        .expect("operation fixture must contain its context entry")
        .clone();
    duplicate.id = fixture::fact_id(600);
    facts.push(duplicate);

    writer
        .append(fixture::id(2), None, facts)
        .expect_err("duplicate context identity must fail before append");
    start_operation(&mut writer);
}

#[test]
fn new_epoch_base_cannot_reference_an_entry_created_in_the_same_commit() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    let epoch_id = fixture::id(700);
    let mut entry = context_entry(
        701,
        ContextRole::System,
        ContextSource::HarnessControl,
        "new brief",
    );
    entry.epoch = epoch_id;
    let epoch = ContextEpoch {
        id: epoch_id,
        head: fixture::head(),
        stage: None,
        base: Items::new(vec![entry.id]).expect("fixture base must fit"),
    };

    writer
        .append(
            fixture::id(2),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(700),
                    data: FactData::ContextEpochOpened { epoch },
                },
                Fact {
                    id: fixture::fact_id(701),
                    data: FactData::ContextEntryAdded { entry },
                },
            ],
        )
        .expect_err("epoch base must reference only prior durable entries");
}

#[test]
fn idle_start_must_project_its_exact_input() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    let mut facts = operation_facts();
    let Some(FactData::ContextEntryAdded { entry }) = facts.last_mut().map(|fact| &mut fact.data)
    else {
        panic!("operation fixture must end with its context entry");
    };
    entry.source = ContextSource::HarnessControl;

    writer
        .append(fixture::id(2), None, facts)
        .expect_err("idle start cannot project unrelated context");
}

#[test]
fn one_commit_cannot_open_two_operations() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    let mut facts = operation_facts();
    facts.extend(operation_facts_for(110, 111, 112, 600));

    writer
        .append(fixture::id(2), None, facts)
        .expect_err("one commit cannot publish two open operations");
}

#[test]
fn one_provider_result_cannot_split_tool_batches() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    start_operation(&mut writer);
    start_provider(&mut writer);
    let (first_payload, first_call) = planned_call(0, 300, 301, 302, "call-1");
    let (second_payload, second_call) = planned_call(1, 310, 311, 312, "call-2");

    writer
        .append(
            fixture::id(4),
            None,
            provider_facts(
                vec![first_payload, second_payload],
                vec![first_call, second_call],
            ),
        )
        .expect_err("one provider result must create one tool batch");
}

pub(super) fn start_operation(writer: &mut JournalWriter) {
    writer
        .append(fixture::id(2), None, operation_facts())
        .expect("operation start group must append");
}

fn operation_facts() -> Vec<Fact> {
    operation_facts_for(100, 101, 102, 3)
}

fn operation_facts_for(input: u128, operation: u128, entry: u128, fact: u128) -> Vec<Fact> {
    let input = fixture::id(input);
    let operation = fixture::id(operation);
    let entry = context_entry(
        entry,
        ContextRole::User,
        ContextSource::Input(input),
        "hello",
    );
    vec![
        Fact {
            id: fixture::fact_id(fact),
            data: FactData::InputAccepted {
                input,
                origin: InputOrigin::User,
                route: InputRoute::Start,
                payload: UserPayload {
                    text: Text::new("hello").expect("fixture input must fit"),
                    images: Items::default(),
                },
                reserved_projection_bytes: 128,
            },
        },
        Fact {
            id: fixture::fact_id(fact + 1),
            data: FactData::InputApplied {
                input,
                operation: Some(operation),
            },
        },
        Fact {
            id: fixture::fact_id(fact + 2),
            data: FactData::OperationOpened {
                operation,
                input,
                work: None,
                stage: None,
            },
        },
        Fact {
            id: fixture::fact_id(fact + 3),
            data: FactData::ContextEntryAdded { entry },
        },
    ]
}

pub(super) fn start_provider(writer: &mut JournalWriter) {
    writer
        .append(
            fixture::id(3),
            None,
            vec![Fact {
                id: fixture::fact_id(7),
                data: FactData::ProviderStarted {
                    intent: provider_intent(),
                },
            }],
        )
        .expect("provider intent must append");
}

fn provider_intent() -> ProviderIntent {
    let request = br#"{"model":"test-model","messages":[{"role":"user","content":"hello"}]}"#;
    let request_digest = Digest::hash(b"mage:provider-request:v1", request);
    ProviderIntent {
        attempt: fixture::id(200),
        result: fixture::id(201),
        operation: fixture::id(101),
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
        through: Some(fixture::id(102)),
        request_bytes: u32::try_from(request.len()).expect("fixture request length must fit"),
        request_digest,
        request_format: 1,
        deadline_unix_millis: 1_800_000_000_000,
        exposure: Exposure {
            max_input_tokens: 1_000,
            max_output_tokens: 1_000,
            max_live_millis: 30_000,
        },
    }
}

pub(super) fn finish_provider_with_tool(writer: &mut JournalWriter) {
    let (payload, call) = planned_call(0, 300, 301, 302, "call-1");
    writer
        .append(
            fixture::id(4),
            None,
            provider_facts(vec![payload], vec![call]),
        )
        .expect("provider terminal and tool obligations must append");
}

pub(super) fn planned_call(
    position: u16,
    id: u128,
    batch: u128,
    result: u128,
    provider_id: &str,
) -> (ProviderToolCall, ToolCall) {
    let provider_call = ProviderToolCall {
        provider_id: fixture::text(provider_id),
        name: fixture::text("read_file"),
        arguments: fixture::text(r#"{"path":"README.md"}"#),
    };
    let contract = ReconciliationContract::None;
    let call = ToolCall {
        id: fixture::id(id),
        batch: fixture::id(batch),
        result: fixture::id(result),
        provider_result: fixture::id(201),
        provider_id: fixture::text(provider_id),
        position,
        registration: fixture::id(303),
        schema_digest: fixture::digest("read-file-schema"),
        effect_contract_digest: contract
            .digest()
            .expect("fixture reconciliation contract must encode"),
    };
    (provider_call, call)
}

pub(super) fn provider_facts(payloads: Vec<ProviderToolCall>, calls: Vec<ToolCall>) -> Vec<Fact> {
    let mut facts = vec![
        Fact {
            id: fixture::fact_id(8),
            data: FactData::ProviderFinished {
                attempt: fixture::id(200),
                result: fixture::id(201),
                outcome: ProviderOutcome::Completed {
                    message: ProviderMessage {
                        text: fixture::text(""),
                        reasoning: fixture::text("I need the file."),
                        tool_calls: Items::new(payloads).expect("fixture tool call must fit"),
                    },
                    stop_reason: StopReason::ToolCalls,
                    reported_usage: usage(),
                },
            },
        },
        Fact {
            id: fixture::fact_id(9),
            data: FactData::MeterSettled {
                meter: fixture::id(202),
                owner: MeterOwner::Provider(fixture::id(200)),
                kind: MeterKind::Provider,
                settlement: MeterSettlement::Known(usage()),
            },
        },
        Fact {
            id: fixture::fact_id(10),
            data: FactData::ContextEntryAdded {
                entry: context_entry(
                    203,
                    ContextRole::Assistant,
                    ContextSource::Provider(fixture::id(201)),
                    "assistant tool call",
                ),
            },
        },
    ];
    facts.extend(calls.into_iter().enumerate().map(|(index, call)| Fact {
        id: fixture::fact_id(11 + u128::try_from(index).expect("fixture index must fit")),
        data: FactData::ToolCallPlanned { call },
    }));
    facts
}

pub(super) fn start_tool(writer: &mut JournalWriter) {
    writer
        .append(
            fixture::id(5),
            None,
            vec![Fact {
                id: fixture::fact_id(12),
                data: FactData::ToolStarted {
                    intent: ToolIntent {
                        attempt: fixture::id(304),
                        call: fixture::id(300),
                        registration: fixture::id(303),
                        normalized_arguments: fixture::text(r#"{"path":"README.md"}"#),
                        decision: PolicyDecision::Allowed,
                        approval: ApprovalSource::Policy(fixture::digest("policy")),
                        replay: ReplayPolicy::ReplaySafe,
                        deadline_unix_millis: 1_800_000_000_000,
                        reservation: fixture::id(305),
                        footprint: WorkspaceFootprint::None,
                        reconciliation: ReconciliationContract::None,
                        process: None,
                    },
                },
            }],
        )
        .expect("tool intent must append");
}

fn finish_tool(writer: &mut JournalWriter) {
    writer
        .append(
            fixture::id(6),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(13),
                    data: FactData::ToolFinished {
                        result: ToolResult {
                            id: fixture::id(302),
                            call: fixture::id(300),
                            attempt: Some(fixture::id(304)),
                            outcome: ToolOutcome::Completed {
                                output: fixture::text("# Mage"),
                                job: None,
                            },
                            workspace: WorkspaceEffect::None,
                        },
                    },
                },
                Fact {
                    id: fixture::fact_id(14),
                    data: FactData::MeterSettled {
                        meter: fixture::id(305),
                        owner: MeterOwner::Tool(fixture::id(304)),
                        kind: MeterKind::Tool,
                        settlement: MeterSettlement::Known(usage()),
                    },
                },
                Fact {
                    id: fixture::fact_id(15),
                    data: FactData::ContextEntryAdded {
                        entry: context_entry(
                            306,
                            ContextRole::Tool,
                            ContextSource::Tool(fixture::id(302)),
                            "# Mage",
                        ),
                    },
                },
            ],
        )
        .expect("tool terminal group must append");
}

fn close_operation(writer: &mut JournalWriter) {
    writer
        .append(
            fixture::id(7),
            None,
            vec![Fact {
                id: fixture::fact_id(16),
                data: FactData::OperationClosed {
                    close: OperationClose {
                        id: fixture::id(101),
                        outcome: OperationOutcome::Completed,
                        provider: Some(fixture::id(201)),
                        work: None,
                        work_status: None,
                        plan: None,
                        candidate: None,
                        verdict: None,
                        usage_digest: fixture::digest("usage"),
                        inputs: Items::default(),
                        diagnostic: None,
                    },
                    stage_outcome: OperationStageOutcome::Stayed,
                },
            }],
        )
        .expect("operation close must append");
}

pub(super) fn context_entry(
    id: u128,
    role: ContextRole,
    source: ContextSource,
    payload: &str,
) -> ContextEntry {
    let mut entry = ContextEntry {
        id: fixture::id(id),
        epoch: fixture::epoch_id(),
        role,
        source,
        payload: fixture::text(payload),
        digest: Digest::default(),
    };
    entry.digest = entry.calculated_digest();
    entry
}

pub(super) fn usage() -> Usage {
    Usage {
        input_tokens: Some(10),
        output_tokens: Some(5),
        cached_input_tokens: Some(0),
        live_millis: Some(100),
    }
}
use std::fs;
