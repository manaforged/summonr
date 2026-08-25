use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::Path;

use super::fixture;
use crate::durable::bound::{Items, Text};
use crate::durable::context::{
    ContextEntry, ContextRole, ContextSource, InputOrigin, InputRoute, OperationClose,
    OperationOutcome, UserPayload,
};
use crate::durable::effect::BudgetKind;
use crate::durable::encoding::Commit;
use crate::durable::fact::{CommandReceipt, CommandStatus, Fact, FactData, OperationStageOutcome};
use crate::durable::id::{Digest, OperationId};
use crate::durable::journal::{JournalError, JournalWriter, find_receipt, inspect};

#[test]
fn actual_journal_replays_and_repairs_only_an_incomplete_tail() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let state = {
        let writer = fixture::writer(&path);
        writer.state().clone()
    };
    let mut file = OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("journal must reopen for fixture corruption");
    file.write_all(b"{\"format\":2")
        .expect("incomplete fixture tail must append");
    drop(file);
    let before = fs::read(&path).expect("journal bytes must be readable");

    assert_eq!(
        inspect(&path).expect("read-only replay ignores tail"),
        state
    );
    assert_eq!(
        fs::read(&path).expect("journal bytes must be readable"),
        before,
        "read-only inspection must not repair bytes"
    );

    let writer = JournalWriter::open(&path).expect("writer open repairs incomplete tail");
    assert_eq!(writer.state(), &state);
    assert!(
        fs::read(&path)
            .expect("journal bytes must be readable")
            .ends_with(b"\n")
    );
}

#[test]
fn complete_corruption_is_never_truncated_as_repair() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    drop(fixture::writer(&path));
    let mut bytes = fs::read(&path).expect("journal bytes must be readable");
    bytes.extend_from_slice(b"{}\n");
    fs::write(&path, &bytes).expect("corrupt fixture must be written");

    inspect(&path).expect_err("corrupt journal must fail inspection");
    let Err(_error) = JournalWriter::open(&path) else {
        panic!("complete corruption must fail writer open");
    };
    assert_eq!(
        fs::read(&path).expect("journal bytes must be readable"),
        bytes
    );
}

#[test]
fn chain_rejects_changed_inserted_removed_and_reordered_lines() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    writer
        .append(
            fixture::id(2),
            None,
            vec![Fact {
                id: fixture::fact_id(3),
                data: FactData::InputAccepted {
                    input: fixture::id(30),
                    origin: InputOrigin::User,
                    route: InputRoute::Queue,
                    payload: UserPayload {
                        text: Text::new("queued").expect("fixture input must fit"),
                        images: Items::default(),
                    },
                },
            }],
        )
        .expect("queued input must append");
    writer
        .append(
            fixture::id(3),
            Some(CommandReceipt {
                id: fixture::id(31),
                digest: fixture::digest("set token budget"),
                status: CommandStatus::Accepted,
                code: None,
                result: Some(fixture::fact_id(4)),
            }),
            vec![Fact {
                id: fixture::fact_id(4),
                data: FactData::BudgetChanged {
                    kind: BudgetKind::SessionTokens,
                    limit: Some(1_000),
                },
            }],
        )
        .expect("token budget must append");
    writer
        .append(
            fixture::id(4),
            Some(CommandReceipt {
                id: fixture::id(32),
                digest: fixture::digest("set live budget"),
                status: CommandStatus::Accepted,
                code: None,
                result: Some(fixture::fact_id(5)),
            }),
            vec![Fact {
                id: fixture::fact_id(5),
                data: FactData::BudgetChanged {
                    kind: BudgetKind::SessionLiveMillis,
                    limit: Some(60_000),
                },
            }],
        )
        .expect("live budget must append");
    drop(writer);

    let original = fs::read_to_string(&path).expect("journal text must be readable");
    let lines: Vec<_> = original.lines().map(str::to_owned).collect();
    assert_eq!(lines.len(), 4);

    let mut changed: serde_json::Value =
        serde_json::from_str(&lines[1]).expect("fixture line must be JSON");
    changed["seq"] = 99.into();
    assert_rejected(
        &path,
        &[&lines[0], &changed.to_string(), &lines[2], &lines[3]],
    );
    assert_rejected(
        &path,
        &[&lines[0], &lines[1], &lines[1], &lines[2], &lines[3]],
    );
    assert_rejected(&path, &[&lines[0], &lines[2], &lines[3]]);
    assert_rejected(&path, &[&lines[0], &lines[2], &lines[1], &lines[3]]);
}

#[test]
fn writer_lock_excludes_a_second_format_2_owner() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let _writer = fixture::writer(&path);
    let error = match JournalWriter::open(&path) {
        Ok(_) => panic!("second writer must be excluded"),
        Err(error) => error,
    };
    assert!(matches!(error, JournalError::Locked { .. }));
}

#[test]
fn prepare_failure_changes_neither_state_nor_journal_bytes() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    let state = writer.state().clone();
    let bytes = fs::read(&path).expect("journal bytes must be readable");
    let result = writer.append(
        fixture::id(2),
        None,
        vec![Fact {
            id: fixture::fact_id(3),
            data: FactData::OperationOpened {
                operation: fixture::id(40),
                input: fixture::id(41),
                work: None,
                stage: None,
            },
        }],
    );
    result.expect_err("invalid transition must fail");
    assert_eq!(writer.state(), &state);
    assert_eq!(
        fs::read(&path).expect("journal bytes must be readable"),
        bytes
    );
}

#[test]
fn partial_append_publishes_no_state_and_poisons_writer() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    let state = writer.state().clone();
    writer.fail_after(7);
    let facts = vec![Fact {
        id: fixture::fact_id(3),
        data: FactData::InputAccepted {
            input: fixture::id(30),
            origin: InputOrigin::User,
            route: InputRoute::Queue,
            payload: UserPayload {
                text: Text::new("queued").expect("fixture input must fit"),
                images: Items::default(),
            },
        },
    }];
    writer
        .append(fixture::id(2), None, facts.clone())
        .expect_err("partial append must fail");
    assert_eq!(writer.state(), &state);
    assert!(matches!(
        writer.append(fixture::id(2), None, facts),
        Err(JournalError::Poisoned)
    ));
    drop(writer);

    assert_eq!(
        inspect(&path).expect("reader ignores partial append"),
        state
    );
    let repaired = JournalWriter::open(&path).expect("writer repairs partial append");
    assert_eq!(repaired.state(), &state);
}

fn assert_rejected(path: &Path, lines: &[&str]) {
    let text = format!("{}\n", lines.join("\n"));
    fs::write(path, text).expect("mutated fixture must be written");
    inspect(path).expect_err("mutated chain must fail");
}

#[test]
fn complete_suffix_rollback_remains_an_honest_chain_limit() {
    let root = Commit::new(1, fixture::id(1), None, None, fixture::root_facts())
        .expect("root commit must encode");
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("prefix.jsonl");
    fs::write(&path, root.line().expect("root line must encode"))
        .expect("valid prefix must be written");
    assert_eq!(inspect(&path).expect("valid prefix must replay").seq(), 1);
}

#[test]
fn cold_receipt_lookup_survives_hot_cache_eviction() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    let mut first = None;

    for index in 0_u128..160 {
        let fact_id = fixture::fact_id(10_000 + index);
        let receipt = CommandReceipt {
            id: fixture::id(20_000 + index),
            digest: fixture::digest(&format!("command-{index}")),
            status: CommandStatus::Accepted,
            code: None,
            result: Some(fact_id),
        };
        first.get_or_insert_with(|| receipt.clone());
        writer
            .append(
                fixture::id(30_000 + index),
                Some(receipt),
                vec![Fact {
                    id: fact_id,
                    data: FactData::BudgetChanged {
                        kind: BudgetKind::SessionTokens,
                        limit: Some(u64::try_from(index).expect("fixture index must fit")),
                    },
                }],
            )
            .expect("receipt fixture must append");
    }

    let first = first.expect("fixture must create a receipt");
    assert_eq!(writer.state().hot_receipts(), 128);
    assert!(writer.state().receipt(first.id).is_none());
    assert!(writer.state().maybe_receipt(first.id));
    assert_eq!(
        writer.receipt(first.id).expect("cold lookup must succeed"),
        Some(first.clone())
    );

    let state = writer.state().clone();
    let bytes = fs::read(&path).expect("journal bytes must be readable");
    writer
        .append(
            fixture::id(40_000),
            Some(first.clone()),
            vec![Fact {
                id: fixture::fact_id(40_001),
                data: FactData::BudgetChanged {
                    kind: BudgetKind::SessionTokens,
                    limit: None,
                },
            }],
        )
        .expect_err("evicted duplicate command must fail");
    assert_eq!(writer.state(), &state);
    assert_eq!(
        fs::read(&path).expect("journal bytes must be readable"),
        bytes
    );
    drop(writer);

    assert_eq!(
        find_receipt(&path, first.id).expect("reopened cold lookup must succeed"),
        Some(first)
    );
}

#[test]
fn closed_operation_history_is_bounded_without_permitting_identity_reuse() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    let first = fixture::id(50_001);

    for index in 0_u128..160 {
        let operation = fixture::id(50_001 + index * 10);
        writer
            .append(
                fixture::id(60_000 + index * 2),
                None,
                operation_start(index, operation),
            )
            .expect("operation fixture must open");
        writer
            .append(
                fixture::id(60_001 + index * 2),
                None,
                operation_close(index, operation),
            )
            .expect("operation fixture must close");
    }

    assert_eq!(writer.state().hot_operations(), 128);
    assert!(writer.state().closed_operation(first).is_none());
    assert!(writer.state().maybe_closed(first));

    let state = writer.state().clone();
    let bytes = fs::read(&path).expect("journal bytes must be readable");
    let duplicate = operation_start(500, first);
    writer
        .append(fixture::id(70_000), None, duplicate.clone())
        .expect_err("evicted operation identity must fail before append");
    assert_eq!(writer.state(), &state);
    assert_eq!(
        fs::read(&path).expect("journal bytes must be readable"),
        bytes
    );
    drop(writer);

    let forged = Commit::new(
        state.seq().saturating_add(1),
        fixture::id(70_000),
        state.head(),
        None,
        duplicate,
    )
    .expect("duplicate operation commit must encode");
    let mut file = OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("journal must open for replay fixture");
    file.write_all(&forged.line().expect("fixture line must encode"))
        .expect("fixture line must append");
    drop(file);

    inspect(&path).expect_err("replay must reject an evicted operation identity");
}

fn operation_start(index: u128, operation: OperationId) -> Vec<Fact> {
    let input = fixture::id(80_000 + index * 10);
    let mut entry = ContextEntry {
        id: fixture::id(80_001 + index * 10),
        epoch: fixture::epoch_id(),
        role: ContextRole::User,
        source: ContextSource::Input(input),
        payload: fixture::text("bounded operation"),
        digest: Digest::default(),
    };
    entry.digest = entry.calculated_digest();
    let fact = 90_000 + index * 10;
    vec![
        Fact {
            id: fixture::fact_id(fact),
            data: FactData::InputAccepted {
                input,
                origin: InputOrigin::User,
                route: InputRoute::Start,
                payload: UserPayload {
                    text: fixture::text("bounded operation"),
                    images: Items::default(),
                },
            },
        },
        Fact {
            id: fixture::fact_id(fact + 1),
            data: FactData::InputApplied { input, operation },
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

fn operation_close(index: u128, operation: OperationId) -> Vec<Fact> {
    vec![Fact {
        id: fixture::fact_id(90_004 + index * 10),
        data: FactData::OperationClosed {
            close: OperationClose {
                id: operation,
                outcome: OperationOutcome::Completed,
                provider: None,
                work: None,
                work_status: None,
                plan: None,
                candidate: None,
                verdict: None,
            },
            stage_outcome: OperationStageOutcome::Stayed,
        },
    }]
}
