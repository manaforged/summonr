use crate::durable::tree::Snapshot;
use crate::durable::work::{
    Decision, Observation, Policy, Stage, Status, StepState, Work, WorkKind, WorkPhase, checks,
};

const PLAN: &str = "## Steps\n\n1. Write a.txt. Run `true # one`.\n2. Write b.txt. Run `true # two`.\n\n## Contract\n\n- file `b.txt`\n- stdout `cat b.txt` → `done`\n";

fn steps_work() -> Work {
    let mut work = Work::new(PLAN, Policy::Steps);
    work.select(PLAN, &["cargo test".to_string()])
        .expect("select");
    work.execute(Snapshot::default()).expect("execute");
    work
}

fn pass(work: &mut Work) {
    work.verify(Snapshot::default()).expect("verify");
    work.verdict(true, Decision::Automatic).expect("verdict");
}

fn fail(work: &mut Work) {
    work.verify(Snapshot::default()).expect("verify");
    work.verdict(false, Decision::Automatic).expect("verdict");
}

#[test]
fn plan_checks_merge_commands_steps_and_contract() {
    let found = checks(PLAN, Policy::Steps, &["cargo test".to_string()]).expect("checks");
    let commands: Vec<&str> = found.iter().map(|check| check.command.as_str()).collect();
    assert_eq!(
        commands,
        [
            "cargo test",
            "true # one",
            "true # two",
            "file exists: b.txt",
            "cat b.txt"
        ]
    );
    assert_eq!(
        found[3].observation,
        Some(Observation::File("b.txt".to_string()))
    );
    assert_eq!(
        found[4].observation,
        Some(Observation::Stdout("done".to_string()))
    );
    let single = checks("anything", Policy::Single, &["true".to_string()]).expect("single");
    assert_eq!(single.len(), 1);
    checks("no steps here", Policy::Steps, &[]).unwrap_err();
}

#[test]
fn steps_own_their_checks_and_the_last_step_owns_all() {
    let work = steps_work();
    assert_eq!(work.steps.len(), 2);
    assert_eq!(work.steps[0].checks, vec![0, 1]);
    assert_eq!(work.steps[1].checks, vec![0, 1, 2, 3, 4]);
    assert_eq!(work.active, Some(0));
    assert_eq!(work.steps[0].state, StepState::Active);
    let owned: Vec<&str> = work
        .completion_checks()
        .iter()
        .map(|check| check.command.as_str())
        .collect::<Vec<_>>()
        .into_iter()
        .map(|_| "")
        .collect();
    assert_eq!(owned.len(), 2);
}

#[test]
fn work_moves_plan_execute_verify_complete_and_cannot_reopen() {
    let mut work = Work::new("ship it", Policy::Single);
    assert_eq!(work.stage(), Some(Stage::Plan));
    assert!(
        work.execute(Snapshot::default()).is_err(),
        "Execute needs a plan"
    );
    work.select("ship it", &["true".to_string()])
        .expect("select");
    work.execute(Snapshot::default()).expect("execute");
    assert_eq!(work.stage(), Some(Stage::Execute));
    assert!(
        work.verdict(true, Decision::Automatic).is_err(),
        "verdict needs Verify"
    );

    let paths = work.verify(Snapshot::default()).expect("verify");
    assert!(paths.is_empty());
    assert_eq!(work.stage(), Some(Stage::Verify));
    work.verdict(true, Decision::Automatic).expect("verdict");
    assert_eq!(work.status, Status::Completed);
    assert!(work.terminal());
    assert_eq!(work.progress().phase, WorkPhase::Completed);

    work.verify(Snapshot::default()).unwrap_err();
    work.abandon("late");
    assert_eq!(
        work.status,
        Status::Completed,
        "terminal Work stays terminal"
    );
}

#[test]
fn failed_verify_returns_to_execute_with_a_repair_queued() {
    let mut work = steps_work();
    fail(&mut work);
    assert_eq!(work.stage(), Some(Stage::Execute));
    assert!(work.candidate.is_none());
    assert_eq!(
        work.pending,
        vec!["Repair the failed completion check.".to_string()]
    );
    assert_eq!(work.steps[0].attempts, 1);
    assert_eq!(work.progress().attempts, 1);
}

#[test]
fn three_failed_checks_abandon_steps_work() {
    let mut work = steps_work();
    for _ in 0..3 {
        fail(&mut work);
    }
    assert!(matches!(work.status, Status::Abandoned(ref reason) if reason.contains("three times")));
    assert_eq!(work.steps[0].state, StepState::Blocked);
    assert_eq!(work.progress().exit_code(), Some(2));
}

#[test]
fn single_work_never_abandons_on_failed_checks() {
    let mut work = Work::new("fix", Policy::Single);
    work.select("fix", &["false".to_string()]).expect("select");
    work.execute(Snapshot::default()).expect("execute");
    for _ in 0..5 {
        fail(&mut work);
    }
    assert_eq!(work.stage(), Some(Stage::Execute));
}

#[test]
fn passed_steps_advance_then_complete() {
    let mut work = steps_work();
    pass(&mut work);
    assert_eq!(work.active, Some(1));
    assert_eq!(work.steps[0].state, StepState::Completed);
    assert_eq!(work.steps[1].state, StepState::Active);
    assert_eq!(work.stage(), Some(Stage::Execute));
    let progress = work.progress();
    assert_eq!(
        (progress.kind, progress.current, progress.total),
        (WorkKind::Steps, Some(2), 2)
    );
    assert_eq!(progress.to_string(), "Work step 2/2 · running");

    pass(&mut work);
    assert_eq!(work.status, Status::Completed);
    assert_eq!(work.progress().current, Some(2));
}

#[test]
fn goal_continues_completes_or_hits_its_turn_limit() {
    let mut work = Work::new("build it", Policy::Goal { max_turns: 2 });
    work.select("build it", &[]).expect("select");
    work.execute(Snapshot::default()).expect("execute");
    work.verify(Snapshot::default()).expect("verify");
    assert!(
        work.verdict(true, Decision::Automatic).is_err(),
        "goal needs a decision"
    );

    work.verdict(true, Decision::GoalContinue("more".to_string()))
        .expect("continue");
    assert_eq!(work.stage(), Some(Stage::Execute));
    assert!(work.pending[0].contains("more"));
    assert_eq!(work.progress().to_string(), "Work goal turn 1/2 · running");

    work.verify(Snapshot::default()).expect("verify");
    work.verdict(true, Decision::GoalContinue("still".to_string()))
        .expect("limit");
    assert!(matches!(work.status, Status::Abandoned(ref reason) if reason.contains("turn limit")));
    assert_eq!(work.progress().exit_code(), Some(3));

    let mut done = Work::new("build it", Policy::Goal { max_turns: 2 });
    done.select("build it", &[]).expect("select");
    done.execute(Snapshot::default()).expect("execute");
    done.verify(Snapshot::default()).expect("verify");
    done.verdict(true, Decision::GoalComplete)
        .expect("complete");
    assert_eq!(done.status, Status::Completed);
}

#[test]
fn interrupt_returns_verify_to_execute_only() {
    let mut work = steps_work();
    work.interrupt();
    assert_eq!(work.stage(), Some(Stage::Execute));
    work.verify(Snapshot::default()).expect("verify");
    work.interrupt();
    assert_eq!(work.stage(), Some(Stage::Execute));
    assert!(work.candidate.is_none());
    assert_eq!(
        work.steps[0].attempts, 0,
        "an interruption is not a failed attempt"
    );
}

#[test]
fn checkpoint_round_trips_and_is_absent_before_work() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("work.json");
    assert_eq!(Work::load(&path).expect("load"), None);
    let mut work = steps_work();
    fail(&mut work);
    work.save(&path).expect("save");
    assert_eq!(Work::load(&path).expect("load"), Some(work));
    assert!(
        !dir.path().join("work.json.tmp").exists(),
        "temp file is renamed away"
    );
    let text = std::fs::read_to_string(&path).expect("read");
    assert!(text.contains("\"policy\": \"steps\""), "{text}");
    assert!(text.contains("\"active\": \"execute\""), "{text}");
}

#[test]
fn review_sits_between_verify_and_the_verdict() {
    let mut work = steps_work();
    work.review().unwrap_err();
    work.verify(Snapshot::default()).expect("verify");
    work.review().expect("review");
    assert_eq!(work.stage(), Some(Stage::Review));
    assert_eq!(work.progress().phase, WorkPhase::Review);
    assert_eq!(work.progress().to_string(), "Work step 1/2 · reviewing");
    work.review().unwrap_err();

    work.verdict(true, Decision::Automatic)
        .expect("a passed review completes the step");
    assert_eq!(work.active, Some(1));
    assert_eq!(work.stage(), Some(Stage::Execute));
}

#[test]
fn a_rejected_review_queues_the_findings_and_counts_an_attempt() {
    let mut work = steps_work();
    work.reject("late").unwrap_err();
    work.verify(Snapshot::default()).expect("verify");
    work.review().expect("review");
    work.reject("rename x to y").expect("reject");
    assert_eq!(work.stage(), Some(Stage::Execute));
    assert!(work.candidate.is_none());
    assert_eq!(
        work.pending,
        vec!["Address the review findings:\nrename x to y".to_string()]
    );
    assert_eq!(work.steps[0].attempts, 1);

    fail(&mut work);
    work.verify(Snapshot::default()).expect("verify");
    work.review().expect("review");
    work.reject("still wrong").expect("reject");
    assert!(matches!(work.status, Status::Abandoned(ref reason) if reason.contains("three times")));
}

#[test]
fn interrupt_returns_review_to_execute() {
    let mut work = steps_work();
    assert!(!work.interrupt());
    work.verify(Snapshot::default()).expect("verify");
    work.review().expect("review");
    assert!(work.interrupt());
    assert_eq!(work.stage(), Some(Stage::Execute));
    assert_eq!(work.steps[0].attempts, 0);
}

#[test]
fn the_brief_travels_with_the_checkpoint() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("work.json");
    let mut work = steps_work();
    assert_eq!(work.brief, None);
    work.brief = Some("touches a.rs".to_string());
    work.save(&path).expect("save");
    assert_eq!(
        Work::load(&path).expect("load").and_then(|work| work.brief),
        Some("touches a.rs".to_string())
    );
}
