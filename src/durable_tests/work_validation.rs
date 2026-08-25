use super::{fixture, work};
use crate::durable::bound::Items;
use crate::durable::work::{WorkRevision, WorkStage};

#[test]
fn candidate_capture_must_name_the_last_execute_revision() {
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
    let mut forged = work::candidate(&plan, &policy, &base, &capture);
    forged.manifest_digest = fixture::digest("forged-manifest");
    forged.digest = forged
        .calculated_digest(base.digest(), capture.digest())
        .expect("fixture candidate must encode");
    writer
        .append(
            fixture::id(7),
            None,
            work::verify_facts(&plan, &policy, capture.clone(), &forged),
        )
        .expect_err("forged candidate manifest must be rejected before append");

    let mut forged = work::candidate(&plan, &policy, &base, &capture);
    forged.execute_revision = WorkRevision::new(2).expect("fixture revision must be valid");
    forged.digest = forged
        .calculated_digest(base.digest(), capture.digest())
        .expect("fixture candidate must encode");
    let state = writer.state().clone();
    let bytes = fs::read(&path).expect("journal bytes must be readable");
    writer
        .append(
            fixture::id(7),
            None,
            work::verify_facts(&plan, &policy, capture.clone(), &forged),
        )
        .expect_err("forged Execute revision must be rejected before append");
    assert_eq!(writer.state(), &state);
    assert_eq!(
        fs::read(&path).expect("journal bytes must be readable"),
        bytes
    );

    let valid = work::candidate(&plan, &policy, &base, &capture);
    work::enter_verify(&mut writer, &plan, &policy, capture, &valid);
}

#[test]
fn execute_base_must_use_the_selected_scope() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut writer = fixture::writer(&path);
    let plan = work::plan_revision();
    work::set_plan(&mut writer, &plan);
    work::finish_materialization(&mut writer, &plan);
    let base = work::observation(506);
    let mut wrong_scope = work::policy();
    wrong_scope.scope = fixture::id(699);
    let (receipt, facts) = work::execute_group(&plan, &base, &wrong_scope);
    writer
        .append(fixture::id(4), Some(receipt), facts)
        .expect_err("Execute base from another scope must be rejected");

    let mut wrong_checks = work::policy();
    wrong_checks.checks = Items::new(vec![fixture::id(698)]).expect("fixture checks must fit");
    let (receipt, facts) = work::execute_group(&plan, &base, &wrong_checks);
    writer
        .append(fixture::id(4), Some(receipt), facts)
        .expect_err("Execute checks must match the selected Plan");
}

#[test]
fn plan_rejects_duplicate_check_ids() {
    let mut plan = work::plan_revision();
    let check = plan
        .checks
        .as_slice()
        .first()
        .expect("fixture Plan must contain a check")
        .clone();
    plan.checks = Items::new(vec![check.clone(), check]).expect("fixture checks must fit");
    plan.digest = plan.calculated_digest().expect("fixture Plan must encode");

    plan.validate()
        .expect_err("Plan check identities must be unique");
}
use std::fs;
