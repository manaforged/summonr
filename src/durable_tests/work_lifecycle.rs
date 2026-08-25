use super::{fixture, work};
use crate::durable::context::ContextSource;
use crate::durable::fact::{Fact, FactData};
use crate::durable::journal::inspect;
use crate::durable::work::{WorkProjection, WorkRevision, WorkStage, WorkStatus};

#[test]
fn work_moves_plan_execute_verify_complete_and_cannot_reopen() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);

    let plan = work::plan_revision();
    work::set_plan(&mut writer, &plan);
    assert!(writer.state().open_materialization.is_some());
    assert_eq!(
        inspect(&path).expect("Plan prefix must replay"),
        *writer.state()
    );

    work::finish_materialization(&mut writer, &plan);
    assert!(writer.state().open_materialization.is_none());

    let base = work::observation(506);
    let policy = work::policy();
    work::enter_execute(&mut writer, &plan, &base, &policy);
    work::open_operation(&mut writer, 6, 610, 611, 612, WorkStage::Execute);

    let capture = work::observation(507);
    let candidate = work::candidate(&plan, &policy, &base, &capture);
    work::enter_verify(&mut writer, &plan, &policy, capture, &candidate);
    work::open_operation(&mut writer, 9, 630, 631, 632, WorkStage::Verify);
    work::start_review(&mut writer);
    writer
        .append(fixture::id(11), None, work::review_terminal())
        .expect_err("completed Verify review cannot omit its candidate close group");

    let completed = work::complete(&mut writer, &plan, &policy, &candidate);
    assert!(completed.status().terminal());
    assert_eq!(
        inspect(&path).expect("completed Work must replay"),
        *writer.state()
    );

    let state = writer.state().clone();
    let bytes = fs::read(&path).expect("journal bytes must be readable");
    let mut reopened = completed;
    reopened.revision = WorkRevision::new(6).expect("revision must be valid");
    reopened.status = WorkStatus::Active(WorkStage::Plan);
    reopened.projection_owner = WorkProjection::Context(fixture::id(650));
    let entry = work::context_entry(650, ContextSource::Work(reopened.id()), "illegal reopen");
    writer
        .append(
            fixture::id(12),
            None,
            vec![
                Fact {
                    id: fixture::fact_id(43),
                    data: FactData::WorkSet {
                        work: Box::new(reopened),
                    },
                },
                Fact {
                    id: fixture::fact_id(44),
                    data: FactData::ContextEntryAdded { entry },
                },
            ],
        )
        .expect_err("terminal Work must not reopen");
    assert_eq!(writer.state(), &state);
    assert_eq!(
        fs::read(&path).expect("journal bytes must be readable"),
        bytes
    );
}
use std::fs;
