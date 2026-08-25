use std::collections::HashSet;

use super::fixture;
use crate::durable::bound::Items;
use crate::durable::context::{ContextEpoch, InputOrigin, InputRoute, UserPayload};
use crate::durable::effect::BudgetKind;
use crate::durable::fact::{CommandReceipt, CommandStatus, Fact, FactData, FactKind};
use crate::durable::id::{ForkId, ScopedId, SessionId};
use crate::durable::journal::{JournalWriter, inspect};
use crate::durable::reducer::SessionState;
use crate::durable::workspace::{PathPlatform, WorkspaceObservation};

#[test]
fn commands_queued_input_and_clear_survive_reopen() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);

    writer
        .append(
            fixture::id(2),
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
        base: Items::default(),
    };
    writer
        .append(
            fixture::id(3),
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
            fixture::id(4),
            None,
            vec![Fact {
                id: fixture::fact_id(1012),
                data: FactData::InputAccepted {
                    input: fixture::id(1013),
                    origin: InputOrigin::User,
                    route: InputRoute::Queue,
                    payload: payload("later"),
                },
            }],
        )
        .expect("queued input must append");
    writer
        .append(
            fixture::id(5),
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
            FactKind::BudgetChanged,
            FactKind::ConversationCleared,
            FactKind::CommandRejected,
        ]
    ));
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
                        observation: before.id(),
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
    assert_eq!(open.observation, before.id());

    let after = WorkspaceObservation::new(
        fixture::id(1155),
        fixture::scope().id(),
        PathPlatform::Unix,
        Vec::new(),
    )
    .expect("fork publication observation must be valid");
    let sealed_journal = fixture::digest("sealed-child-journal");
    let observation = after.id();
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
                        sealed_journal,
                        observation,
                    },
                },
            ],
        )
        .expect("published fork terminal must append");
    assert_eq!(
        writer.state().forks.get(&fixture::id(1154)),
        Some(&(sealed_journal, observation))
    );
    assert_eq!(
        inspect(&path).expect("published fork must replay"),
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
                data: FactData::ForkSnapshotSealed { intent, preceding },
            }],
        )
        .expect("child snapshot seal must append");
    assert_eq!(
        inspect(&path).expect("sealed child must replay"),
        *writer.state()
    );
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

fn has(state: &SessionState, expected: &[FactKind]) -> bool {
    let kinds: HashSet<_> = state.facts.values().copied().collect();
    expected.iter().all(|kind| kinds.contains(kind))
}
