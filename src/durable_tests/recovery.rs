use std::collections::HashSet;

use super::{fixture, reducer};
use crate::durable::bound::Items;
use crate::durable::context::{
    AbortReason, ContextEntry, ContextEpoch, ContextRole, ContextSource, Directive, DirectiveState,
    InputOrigin, InputRoute, OperationClose, OperationOutcome, RetryIntent, RetryOutcome,
    RetryReason, RouteReason, UserPayload,
};
use crate::durable::effect::{
    BudgetKind, Exposure, MeterKind, MeterSettlement, ProviderFailureKind, ProviderIntent,
    ProviderOutcome, ProviderPurpose, Usage,
};
use crate::durable::fact::{
    CommandReceipt, CommandStatus, Fact, FactData, FactKind, ForkAdoption, ForkOutcome,
    ImportedWorkStatus, LegacySource, MeterOwner, OperationStageOutcome,
};
use crate::durable::id::{ContextEpochId, Digest, ForkId, ScopedId, SessionId};
use crate::durable::journal::{JournalWriter, inspect};
use crate::durable::reducer::SessionState;
use crate::durable::workspace::{PathPlatform, WorkspaceObservation};

#[test]
fn commands_directives_routes_and_clear_survive_reopen() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);

    writer
        .append(
            fixture::id(2),
            None,
            vec![Fact {
                id: fixture::fact_id(1000),
                data: FactData::InputAccepted {
                    input: fixture::id(1001),
                    origin: InputOrigin::User,
                    route: InputRoute::Steer,
                    payload: payload("Keep the reducer pure."),
                    reserved_projection_bytes: 128,
                },
            }],
        )
        .expect("steering input must append");
    let directive = Directive {
        id: fixture::id(1002),
        text: fixture::text("Keep the reducer pure."),
        state: DirectiveState::Active,
    };
    writer
        .append(
            fixture::id(3),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(1003),
                    data: FactData::InputApplied {
                        input: fixture::id(1001),
                        operation: None,
                    },
                },
                Fact {
                    id: fixture::fact_id(1004),
                    data: FactData::DirectiveChanged {
                        directive: directive.clone(),
                    },
                },
                Fact {
                    id: fixture::fact_id(1005),
                    data: FactData::ContextEntryAdded {
                        entry: entry(
                            1006,
                            fixture::epoch_id(),
                            ContextSource::Directive(directive.id),
                            "Keep the reducer pure.",
                        ),
                    },
                },
            ],
        )
        .expect("steering group must append");

    writer
        .append(
            fixture::id(4),
            Some(accepted(1007, 1008, "budget")),
            vec![Fact {
                id: fixture::fact_id(1008),
                data: FactData::BudgetChanged {
                    kind: BudgetKind::SessionTokens,
                    limit: Some(100_000),
                },
            }],
        )
        .expect("budget command must append");

    let replacement = ContextEpoch {
        id: fixture::id(1009),
        head: fixture::head(),
        stage: None,
        base: Items::new(vec![fixture::id(1006)]).expect("epoch base must fit"),
    };
    writer
        .append(
            fixture::id(5),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(1010),
                    data: FactData::ConversationCleared {
                        previous_epoch: fixture::epoch_id(),
                        replacement_epoch: replacement.id,
                    },
                },
                Fact {
                    id: fixture::fact_id(1011),
                    data: FactData::ContextEpochOpened { epoch: replacement },
                },
            ],
        )
        .expect("clear group must append");

    writer
        .append(
            fixture::id(6),
            None,
            vec![Fact {
                id: fixture::fact_id(1012),
                data: FactData::InputAccepted {
                    input: fixture::id(1013),
                    origin: InputOrigin::User,
                    route: InputRoute::Queue,
                    payload: payload("later"),
                    reserved_projection_bytes: 32,
                },
            }],
        )
        .expect("queued input must append");
    writer
        .append(
            fixture::id(7),
            None,
            vec![Fact {
                id: fixture::fact_id(1014),
                data: FactData::InputRouted {
                    input: fixture::id(1013),
                    route: InputRoute::Steer,
                    reason: RouteReason::User,
                },
            }],
        )
        .expect("input route must append");
    writer
        .append(
            fixture::id(8),
            None,
            vec![Fact {
                id: fixture::fact_id(1015),
                data: FactData::InputCancelled {
                    input: fixture::id(1013),
                    diagnostic: None,
                },
            }],
        )
        .expect("input cancellation must append");
    writer
        .append(
            fixture::id(9),
            Some(CommandReceipt {
                id: fixture::id(1016),
                digest: fixture::digest("busy-command"),
                status: CommandStatus::Rejected,
                code: Some(fixture::text("busy")),
                result: Some(fixture::fact_id(1017)),
            }),
            vec![Fact {
                id: fixture::fact_id(1017),
                data: FactData::CommandRejected {
                    code: fixture::text("busy"),
                    diagnostic: fixture::text("An operation is already open."),
                },
            }],
        )
        .expect("state-dependent rejection must append");

    assert_eq!(
        inspect(&path).expect("command history must replay"),
        *writer.state()
    );
    assert!(has(
        writer.state(),
        &[
            FactKind::InputAccepted,
            FactKind::InputRouted,
            FactKind::InputApplied,
            FactKind::InputCancelled,
            FactKind::DirectiveChanged,
            FactKind::BudgetChanged,
            FactKind::ConversationCleared,
            FactKind::CommandRejected,
        ]
    ));
}

#[test]
fn fork_intent_cancel_and_terminal_are_durable_prefixes() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    let observation = WorkspaceObservation::new(
        fixture::id(1100),
        fixture::scope().id(),
        PathPlatform::Unix,
        Vec::new(),
    )
    .expect("fork observation must be valid");
    let source_seq = writer.state().seq();
    let source_digest = writer.state().head().expect("root must have a chain head");
    writer
        .append(
            fixture::id(2),
            Some(accepted(1101, 1103, "fork")),
            vec![
                Fact {
                    id: fixture::fact_id(1102),
                    data: FactData::WorkspaceObserved {
                        observation: observation.clone(),
                    },
                },
                Fact {
                    id: fixture::fact_id(1103),
                    data: FactData::ForkPlanned {
                        intent: fixture::id(1104),
                        child: SessionId::from_u128(2),
                        source_seq,
                        source_digest,
                        adoption: ForkAdoption::RemainOnParent,
                        observation: observation.id(),
                        plan_footprint: None,
                    },
                },
            ],
        )
        .expect("fork intent must append");
    assert_eq!(
        inspect(&path).expect("fork intent must replay"),
        *writer.state()
    );

    writer
        .append(
            fixture::id(3),
            Some(accepted(1105, 1106, "cancel-fork")),
            vec![Fact {
                id: fixture::fact_id(1106),
                data: FactData::ForkCancelRequested {
                    intent: fixture::id(1104),
                },
            }],
        )
        .expect("fork cancel intent must append");
    writer
        .append(
            fixture::id(4),
            None,
            vec![Fact {
                id: fixture::fact_id(1107),
                data: FactData::ForkFinished {
                    intent: fixture::id(1104),
                    outcome: ForkOutcome::Cancelled,
                },
            }],
        )
        .expect("fork terminal must append");
    assert!(writer.state().open_fork.is_none());
}

#[test]
fn published_fork_retains_its_recovery_intent_until_terminal() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    let before = WorkspaceObservation::new(
        fixture::id(1150),
        fixture::scope().id(),
        PathPlatform::Unix,
        Vec::new(),
    )
    .expect("fork source observation must be valid");
    let source_seq = writer.state().seq();
    let source_digest = writer.state().head().expect("root must have a chain head");
    writer
        .append(
            fixture::id(2),
            Some(accepted(1151, 1153, "publish-fork")),
            vec![
                Fact {
                    id: fixture::fact_id(1152),
                    data: FactData::WorkspaceObserved {
                        observation: before.clone(),
                    },
                },
                Fact {
                    id: fixture::fact_id(1153),
                    data: FactData::ForkPlanned {
                        intent: fixture::id(1154),
                        child: SessionId::from_u128(3),
                        source_seq,
                        source_digest,
                        adoption: ForkAdoption::AdoptChild,
                        observation: before.id(),
                        plan_footprint: None,
                    },
                },
            ],
        )
        .expect("fork publication intent must append");
    let open = writer
        .state()
        .open_fork
        .as_ref()
        .expect("fork recovery intent must remain open");
    assert_eq!(open.id, fixture::id(1154));
    assert_eq!(open.child, SessionId::from_u128(3));
    assert_eq!(open.source_seq, source_seq);
    assert_eq!(open.source_digest, source_digest);
    assert_eq!(open.adoption, ForkAdoption::AdoptChild);
    assert_eq!(open.observation, before.id());
    assert!(open.plan_footprint.is_none());

    let after = WorkspaceObservation::new(
        fixture::id(1155),
        fixture::scope().id(),
        PathPlatform::Unix,
        Vec::new(),
    )
    .expect("fork publication observation must be valid");
    let outcome = ForkOutcome::Published {
        sealed_journal: fixture::digest("sealed-child-journal"),
        observation: after.id(),
    };
    writer
        .append(
            fixture::id(3),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(1156),
                    data: FactData::WorkspaceObserved { observation: after },
                },
                Fact {
                    id: fixture::fact_id(1157),
                    data: FactData::ForkFinished {
                        intent: fixture::id(1154),
                        outcome: outcome.clone(),
                    },
                },
            ],
        )
        .expect("published fork terminal must append");
    assert_eq!(writer.state().forks.get(&fixture::id(1154)), Some(&outcome));
    assert_eq!(
        inspect(&path).expect("published fork must replay"),
        *writer.state()
    );
}

#[test]
fn legacy_root_and_source_acceptance_replay_without_legacy_io() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("legacy.jsonl");
    let mut writer = JournalWriter::create(&path).expect("journal must be created");
    let observation = WorkspaceObservation::new(
        fixture::id(1200),
        fixture::scope().id(),
        PathPlatform::Unix,
        Vec::new(),
    )
    .expect("import observation must be valid");
    let source = legacy_source(10);
    writer
        .append(
            fixture::id(1),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(1201),
                    data: FactData::SessionCreated {
                        session: fixture::session(),
                        scope: fixture::scope(),
                    },
                },
                Fact {
                    id: fixture::fact_id(1202),
                    data: FactData::LegacyImported {
                        source: source.clone(),
                        plan: None,
                        work: ImportedWorkStatus::None,
                        observation: observation.id(),
                        uncertain_tools: Items::default(),
                    },
                },
                Fact {
                    id: fixture::fact_id(1203),
                    data: FactData::WorkspaceObserved { observation },
                },
                Fact {
                    id: fixture::fact_id(1204),
                    data: FactData::ContextEpochOpened {
                        epoch: fixture::epoch(),
                    },
                },
            ],
        )
        .expect("legacy publication root must append");
    writer
        .append(
            fixture::id(2),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(1205),
                    data: FactData::ContextEntryAdded {
                        entry: entry(
                            1206,
                            fixture::epoch_id(),
                            ContextSource::HarnessControl,
                            "legacy tool result unavailable",
                        ),
                    },
                },
                Fact {
                    id: fixture::fact_id(1207),
                    data: FactData::LegacyToolUncertain {
                        provider_call: fixture::text("legacy-call"),
                        result: fixture::id(1208),
                        context: fixture::id(1206),
                    },
                },
            ],
        )
        .expect("legacy unmatched tool must close synthetically");
    writer
        .append(
            fixture::id(3),
            Some(accepted(1209, 1210, "accept-legacy")),
            vec![Fact {
                id: fixture::fact_id(1210),
                data: FactData::LegacySourceAccepted {
                    source: legacy_source(12),
                },
            }],
        )
        .expect("legacy source acceptance must append");
    assert_eq!(
        inspect(&path).expect("legacy history must replay"),
        *writer.state()
    );
}

#[test]
fn fork_child_root_seals_without_parent_state_reads() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("child.jsonl");
    let mut writer = JournalWriter::create(&path).expect("child journal must be created");
    let source = SessionId::from_u128(2);
    let intent: ForkId = ScopedId::new(source, 77);
    writer
        .append(
            fixture::id(1),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(1220),
                    data: FactData::SessionCreated {
                        session: fixture::session(),
                        scope: fixture::scope(),
                    },
                },
                Fact {
                    id: fixture::fact_id(1221),
                    data: FactData::ForkedFrom {
                        source,
                        source_seq: 10,
                        source_digest: fixture::digest("parent-head"),
                        intent,
                        inherited_terminal_seq: 10,
                    },
                },
                Fact {
                    id: fixture::fact_id(1222),
                    data: FactData::ContextEpochOpened {
                        epoch: fixture::epoch(),
                    },
                },
            ],
        )
        .expect("child publication root must append");
    let preceding = writer
        .state()
        .head()
        .expect("child root must have a chain head");
    writer
        .append(
            fixture::id(2),
            None,
            vec![Fact {
                id: fixture::fact_id(1223),
                data: FactData::ForkSnapshotSealed {
                    intent,
                    preceding,
                    next_request: None,
                    inherited_terminal_seq: 10,
                },
            }],
        )
        .expect("child snapshot seal must append");
    assert_eq!(
        inspect(&path).expect("sealed child must replay"),
        *writer.state()
    );
}

#[test]
fn retry_abort_and_interrupted_provider_settle_once() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    reducer::start_operation(&mut writer);
    reducer::start_provider(&mut writer);
    writer
        .append(fixture::id(4), None, provider_failure(1300))
        .expect("provider failure must settle");
    writer
        .append(
            fixture::id(5),
            None,
            vec![Fact {
                id: fixture::fact_id(1302),
                data: FactData::RetryScheduled {
                    retry: RetryIntent {
                        id: fixture::id(1303),
                        operation: fixture::id(101),
                        reservation: fixture::id(1304),
                        reason: RetryReason::RateLimit,
                        not_before_unix_millis: 10,
                        deadline_unix_millis: 20,
                        next_request_digest: fixture::digest("retry-request"),
                    },
                },
            }],
        )
        .expect("retry intent must append");

    let request = br#"{"model":"test-model","messages":[]}"#;
    writer
        .append(
            fixture::id(6),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(1305),
                    data: FactData::RetryFinished {
                        retry: fixture::id(1303),
                        outcome: RetryOutcome::Elapsed,
                    },
                },
                Fact {
                    id: fixture::fact_id(1306),
                    data: FactData::MeterSettled {
                        meter: fixture::id(1304),
                        owner: MeterOwner::Retry(fixture::id(1303)),
                        kind: MeterKind::Retry,
                        settlement: MeterSettlement::Known(usage()),
                    },
                },
                Fact {
                    id: fixture::fact_id(1307),
                    data: FactData::ProviderStarted {
                        intent: ProviderIntent {
                            attempt: fixture::id(1308),
                            result: fixture::id(1309),
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
                            request_bytes: u32::try_from(request.len())
                                .expect("request length must fit"),
                            request_digest: Digest::hash(b"mage:provider-request:v1", request),
                            request_format: 1,
                            deadline_unix_millis: 30,
                            exposure: Exposure {
                                max_input_tokens: 100,
                                max_output_tokens: 100,
                                max_live_millis: 1_000,
                            },
                        },
                    },
                },
            ],
        )
        .expect("elapsed retry must start exactly one next provider");
    writer
        .append(
            fixture::id(7),
            None,
            vec![Fact {
                id: fixture::fact_id(1310),
                data: FactData::AbortRequested {
                    operation: fixture::id(101),
                    reason: AbortReason::User,
                },
            }],
        )
        .expect("abort intent must append");
    writer
        .append(
            fixture::id(8),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(1311),
                    data: FactData::ProviderFinished {
                        attempt: fixture::id(1308),
                        result: fixture::id(1309),
                        outcome: ProviderOutcome::Interrupted {
                            reason: fixture::text("cancelled"),
                            partial_usage: usage(),
                        },
                    },
                },
                Fact {
                    id: fixture::fact_id(1312),
                    data: FactData::MeterSettled {
                        meter: fixture::id(1313),
                        owner: MeterOwner::Provider(fixture::id(1308)),
                        kind: MeterKind::Provider,
                        settlement: MeterSettlement::Unknown(Exposure {
                            max_input_tokens: 100,
                            max_output_tokens: 100,
                            max_live_millis: 1_000,
                        }),
                    },
                },
            ],
        )
        .expect("interrupted provider must settle once");
    writer
        .append(
            fixture::id(9),
            None,
            vec![Fact {
                id: fixture::fact_id(1314),
                data: FactData::OperationClosed {
                    close: OperationClose {
                        id: fixture::id(101),
                        outcome: OperationOutcome::Cancelled,
                        provider: Some(fixture::id(1309)),
                        work: None,
                        work_status: None,
                        plan: None,
                        candidate: None,
                        verdict: None,
                        usage_digest: fixture::digest("cancelled-usage"),
                        inputs: Items::default(),
                        diagnostic: None,
                    },
                    stage_outcome: OperationStageOutcome::Stayed,
                },
            }],
        )
        .expect("cancelled operation must close");
    assert_eq!(
        inspect(&path).expect("retry and abort must replay"),
        *writer.state()
    );
}

#[test]
fn cancelled_retry_settles_and_closes_in_one_commit() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    reducer::start_operation(&mut writer);
    reducer::start_provider(&mut writer);
    writer
        .append(fixture::id(4), None, provider_failure(1400))
        .expect("provider failure must settle");
    writer
        .append(
            fixture::id(5),
            None,
            vec![Fact {
                id: fixture::fact_id(1402),
                data: FactData::RetryScheduled {
                    retry: RetryIntent {
                        id: fixture::id(1403),
                        operation: fixture::id(101),
                        reservation: fixture::id(1404),
                        reason: RetryReason::Transport,
                        not_before_unix_millis: 10,
                        deadline_unix_millis: 20,
                        next_request_digest: fixture::digest("cancelled-retry-request"),
                    },
                },
            }],
        )
        .expect("retry intent must append");
    writer
        .append(
            fixture::id(6),
            None,
            vec![Fact {
                id: fixture::fact_id(1405),
                data: FactData::AbortRequested {
                    operation: fixture::id(101),
                    reason: AbortReason::User,
                },
            }],
        )
        .expect("retry cancellation intent must append");
    writer
        .append(
            fixture::id(7),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(1406),
                    data: FactData::RetryFinished {
                        retry: fixture::id(1403),
                        outcome: RetryOutcome::Cancelled,
                    },
                },
                Fact {
                    id: fixture::fact_id(1407),
                    data: FactData::MeterSettled {
                        meter: fixture::id(1404),
                        owner: MeterOwner::Retry(fixture::id(1403)),
                        kind: MeterKind::Retry,
                        settlement: MeterSettlement::Known(usage()),
                    },
                },
                Fact {
                    id: fixture::fact_id(1408),
                    data: FactData::OperationClosed {
                        close: OperationClose {
                            id: fixture::id(101),
                            outcome: OperationOutcome::Cancelled,
                            provider: Some(fixture::id(201)),
                            work: None,
                            work_status: None,
                            plan: None,
                            candidate: None,
                            verdict: None,
                            usage_digest: fixture::digest("cancelled-retry-usage"),
                            inputs: Items::default(),
                            diagnostic: None,
                        },
                        stage_outcome: OperationStageOutcome::Stayed,
                    },
                },
            ],
        )
        .expect("cancelled retry must settle with operation close");
    assert_eq!(
        inspect(&path).expect("cancelled retry must replay"),
        *writer.state()
    );
}

fn provider_failure(meter: u128) -> Vec<Fact> {
    vec![
        Fact {
            id: fixture::fact_id(1300),
            data: FactData::ProviderFinished {
                attempt: fixture::id(200),
                result: fixture::id(201),
                outcome: ProviderOutcome::Failed {
                    kind: ProviderFailureKind::RateLimit,
                    diagnostic: fixture::text("rate limited"),
                    partial_usage: usage(),
                },
            },
        },
        Fact {
            id: fixture::fact_id(1301),
            data: FactData::MeterSettled {
                meter: fixture::id(meter),
                owner: MeterOwner::Provider(fixture::id(200)),
                kind: MeterKind::Provider,
                settlement: MeterSettlement::Known(usage()),
            },
        },
    ]
}

fn entry(id: u128, epoch: ContextEpochId, source: ContextSource, text: &str) -> ContextEntry {
    let mut entry = ContextEntry {
        id: fixture::id(id),
        epoch,
        role: ContextRole::User,
        source,
        payload: fixture::text(text),
        digest: Digest::default(),
    };
    entry.digest = entry.calculated_digest();
    entry
}

fn payload(text: &str) -> UserPayload {
    UserPayload {
        text: fixture::text(text),
        images: Items::default(),
    }
}

fn accepted(command: u128, result: u128, label: &str) -> CommandReceipt {
    CommandReceipt {
        id: fixture::id(command),
        digest: fixture::digest(label),
        status: CommandStatus::Accepted,
        code: None,
        result: Some(fixture::fact_id(result)),
    }
}

fn legacy_source(length: u64) -> LegacySource {
    LegacySource {
        display_id: fixture::text("legacy-display-id"),
        format: 1,
        converter: 1,
        log_length: length,
        log_digest: fixture::digest(&format!("legacy-{length}")),
        plan_length: None,
        plan_digest: None,
    }
}

fn usage() -> Usage {
    Usage {
        input_tokens: Some(10),
        output_tokens: Some(5),
        cached_input_tokens: None,
        live_millis: Some(100),
    }
}

fn has(state: &SessionState, expected: &[FactKind]) -> bool {
    let kinds: HashSet<_> = state.facts.values().copied().collect();
    expected.iter().all(|kind| kinds.contains(kind))
}
