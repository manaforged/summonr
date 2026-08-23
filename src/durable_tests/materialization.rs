use super::{fixture, work};
use crate::durable::bound::Items;
use crate::durable::context::ContextSource;
use crate::durable::fact::{CommandReceipt, CommandStatus, Fact, FactData, MaterializationOutcome};
use crate::durable::id::Digest;
use crate::durable::journal::{JournalWriter, Result as JournalResult, inspect};
use crate::durable::work::{WorkProjection, WorkStage, WorkStatus};

#[test]
fn plan_materialization_retry_requires_a_visible_failed_outcome() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    let plan = work::plan_revision();
    work::set_plan(&mut writer, &plan);

    terminal(
        &mut writer,
        3,
        30,
        502,
        MaterializationOutcome::Conflict {
            observed: fixture::digest("divergent-plan"),
        },
        2,
        false,
    );
    retry(&mut writer, 4, 40, 505).expect("conflicted Plan can retry by command");
    assert_eq!(
        inspect(&path).expect("retry intent must replay"),
        *writer.state()
    );

    terminal(
        &mut writer,
        5,
        50,
        505,
        MaterializationOutcome::Matched,
        3,
        true,
    );
    retry(&mut writer, 6, 60, 506).expect_err("usable Plan cannot be retried");
}

fn retry(
    writer: &mut JournalWriter,
    commit: u128,
    fact: u128,
    attempt: u128,
) -> JournalResult<Digest> {
    let plan = work::plan_revision();
    let fact_id = fixture::fact_id(fact);
    writer.append(
        fixture::id(commit),
        Some(CommandReceipt {
            id: fixture::id(700 + commit),
            digest: fixture::digest(&format!("retry-materialization-{commit}")),
            status: CommandStatus::Accepted,
            code: None,
            result: Some(fact_id),
        }),
        vec![Fact {
            id: fact_id,
            data: FactData::PlanMaterializationPlanned {
                attempt: fixture::id(attempt),
                plan: plan.id(),
                digest: plan.digest(),
                footprint: work::publication(&plan, fixture::id(attempt)),
            },
        }],
    )
}

fn terminal(
    writer: &mut JournalWriter,
    commit: u128,
    fact: u128,
    attempt: u128,
    outcome: MaterializationOutcome,
    revision: u64,
    selected: bool,
) {
    let plan = work::plan_revision();
    let entry_id = 500 + fact;
    let entry = work::context_entry(
        entry_id,
        ContextSource::Work(plan.work()),
        "Plan materialization inspected",
    );
    writer
        .append(
            fixture::id(commit),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(fact),
                    data: FactData::PlanMaterializationFinished {
                        attempt: fixture::id(attempt),
                        plan: plan.id(),
                        outcome,
                    },
                },
                Fact {
                    id: fixture::fact_id(fact + 1),
                    data: FactData::WorkSet {
                        work: Box::new(work::work_state(
                            revision,
                            WorkStatus::Active(WorkStage::Plan),
                            selected.then_some(plan.id()),
                            None,
                            None,
                            None,
                            None,
                            WorkProjection::Context(fixture::id(entry_id)),
                            Items::default(),
                        )),
                    },
                },
                Fact {
                    id: fixture::fact_id(fact + 2),
                    data: FactData::ContextEntryAdded { entry },
                },
            ],
        )
        .expect("materialization outcome must append");
}
