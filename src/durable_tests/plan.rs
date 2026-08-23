use super::{fixture, work};
use crate::durable::bound::Items;
use crate::durable::context::ContextEpoch;
use crate::durable::fact::{Fact, FactData, OperationStageOutcome};
use crate::durable::journal::inspect;
use crate::durable::work::{PlanSource, WorkProjection, WorkStage, WorkStatus};

#[test]
fn completed_plan_reply_is_one_durable_group() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    let epoch = ContextEpoch {
        id: fixture::id(800),
        head: fixture::head(),
        stage: Some(WorkStage::Plan),
        base: Items::default(),
    };
    writer
        .append(
            fixture::id(2),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(800),
                    data: FactData::WorkSet {
                        work: Box::new(work::work_state(
                            1,
                            WorkStatus::Active(WorkStage::Plan),
                            None,
                            None,
                            None,
                            None,
                            None,
                            WorkProjection::Epoch(epoch.id),
                            Items::default(),
                        )),
                    },
                },
                Fact {
                    id: fixture::fact_id(801),
                    data: FactData::ContextEpochOpened { epoch },
                },
            ],
        )
        .expect("Plan Work must open its stage epoch");
    work::open_operation(&mut writer, 3, 801, 802, 803, WorkStage::Plan);
    work::start_plan_provider(&mut writer);

    let mut plan = work::plan_revision();
    plan.source = PlanSource::Generated {
        provider: fixture::id(820),
    };
    plan.digest = plan
        .calculated_digest()
        .expect("generated Plan must encode");
    writer
        .append(fixture::id(5), None, work::plan_provider_terminal())
        .expect_err("Plan provider terminal cannot omit its durable Plan group");

    let mut facts = work::plan_provider_terminal();
    facts.extend([
        Fact {
            id: fixture::fact_id(833),
            data: FactData::PlanSet { plan: plan.clone() },
        },
        Fact {
            id: fixture::fact_id(834),
            data: FactData::PlanMaterializationPlanned {
                attempt: fixture::id(821),
                plan: plan.id(),
                digest: plan.digest(),
                footprint: work::publication(&plan, fixture::id(821)),
            },
        },
        Fact {
            id: fixture::fact_id(835),
            data: FactData::WorkSet {
                work: Box::new(work::work_state(
                    2,
                    WorkStatus::Active(WorkStage::Plan),
                    Some(plan.id()),
                    None,
                    None,
                    None,
                    None,
                    WorkProjection::Plan(plan.id()),
                    Items::default(),
                )),
            },
        },
        Fact {
            id: fixture::fact_id(836),
            data: FactData::OperationClosed {
                close: work::close(
                    802,
                    Some(fixture::id(820)),
                    WorkStatus::Active(WorkStage::Plan),
                    None,
                    None,
                ),
                stage_outcome: OperationStageOutcome::Stayed,
            },
        },
    ]);
    writer
        .append(fixture::id(5), None, facts)
        .expect("completed Plan reply must append atomically");
    assert_eq!(
        inspect(&path).expect("completed Plan reply must replay"),
        *writer.state()
    );
}
