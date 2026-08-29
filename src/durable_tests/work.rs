use crate::durable::tree::Snapshot;
use crate::durable::work::{
    Decision, Observation, Policy, Stage, Status, StepState, Work, WorkKind, WorkPhase, checks,
    roman,
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

#[test]
fn goal_stops_on_a_stop_decision_or_three_idle_turns() {
    let mut work = Work::new("build it", Policy::Goal { max_turns: 9 });
    work.select("build it", &[]).expect("select");
    work.execute(Snapshot::default()).expect("execute");
    work.verify(Snapshot::default()).expect("verify");
    work.verdict(
        true,
        Decision::GoalStop("impossible: no network".to_string()),
    )
    .expect("stop");
    assert_eq!(
        work.status,
        Status::Abandoned("impossible: no network".to_string())
    );
    assert_eq!(work.steps[0].state, StepState::Blocked);
    assert_eq!(work.progress().exit_code(), Some(3));

    let mut idle = Work::new("build it", Policy::Goal { max_turns: 9 });
    idle.select("build it", &[]).expect("select");
    idle.execute(Snapshot::default()).expect("execute");
    for turn in 1..=3 {
        idle.verify(Snapshot::default()).expect("verify");
        idle.verdict(true, Decision::GoalContinue("more".to_string()))
            .expect("continue");
        assert_eq!(idle.idle, turn);
    }
    assert!(matches!(idle.status, Status::Abandoned(ref reason) if reason.starts_with("stalled:")));
    assert_eq!(idle.progress().exit_code(), Some(3));
}

#[test]
fn ledger_groups_steps_under_roman_phases() {
    let plan = "## Steps\n\n### Setup\n\n1. Make a.\n2. Make b.\n\n### Tests\n\n3. Test both.\n";
    let mut work = Work::new(plan, Policy::Steps);
    work.select(plan, &[]).expect("select");
    work.execute(Snapshot::default()).expect("execute");
    pass(&mut work);
    assert_eq!(
        work.progress().ledger,
        vec![
            "I. Setup",
            "  [x] Make a.",
            "  [/] Make b.",
            "II. Tests",
            "  [ ] Test both."
        ]
    );
    work.abandon("late");
    assert_eq!(work.progress().ledger[2], "  [!] Make b.");
    assert!(steps_work().progress().ledger[0].starts_with("[/] Write a.txt"));
    assert!(Work::new("x", Policy::Single).progress().ledger.is_empty());
    assert_eq!(roman(4), "IV");
    assert_eq!(roman(14), "XIV");
    assert_eq!(roman(49), "XLIX");
    assert_eq!(roman(99), "XCIX");
    assert_eq!(roman(2024), "MMXXIV");
}

#[test]
fn stall_fuse_counts_turns_since_the_last_change() {
    let mut work = Work::new("build it", Policy::Goal { max_turns: 9 });
    work.select("build it", &[]).expect("select");
    work.execute(Snapshot::default()).expect("execute");
    let edited = Snapshot::with(&["a.rs"]);
    work.verify(edited.clone()).expect("verify");
    assert_eq!(work.idle, 0, "an edit is not idle");
    work.verdict(true, Decision::GoalContinue("more".to_string()))
        .expect("continue");
    assert_eq!(
        work.base.as_ref(),
        Some(&edited),
        "the base advances to the accepted capture"
    );
    for turn in 1..=3 {
        work.verify(edited.clone()).expect("verify");
        work.verdict(true, Decision::GoalContinue("more".to_string()))
            .expect("continue");
        assert_eq!(work.idle, turn);
    }
    assert!(matches!(work.status, Status::Abandoned(ref reason) if reason.starts_with("stalled:")));

    let mut steps = steps_work();
    let first = Snapshot::with(&["a.txt"]);
    steps.verify(first.clone()).expect("verify");
    steps
        .verdict(true, Decision::Automatic)
        .expect("step one passes");
    assert_eq!(
        steps.base.as_ref(),
        Some(&first),
        "each step diffs against the last"
    );
}

#[test]
fn touched_paths_accumulate_across_turns_including_failed_ones() {
    let mut work = steps_work();
    work.verify(Snapshot::with(&["a.txt"])).expect("verify");
    work.verdict(false, Decision::Automatic).expect("fail");
    work.verify(Snapshot::with(&["a.txt", "b.txt"]))
        .expect("verify");
    work.verdict(true, Decision::Automatic).expect("pass");
    assert_eq!(work.progress().touched, vec!["a.txt", "b.txt"]);
}

#[test]
fn an_edit_the_snapshot_cannot_see_still_counts_as_progress() {
    let mut work = Work::new("build it", Policy::Goal { max_turns: 9 });
    work.select("build it", &[]).expect("select");
    work.execute(Snapshot::default()).expect("execute");
    work.mark_edit();
    work.verify(Snapshot::default()).expect("verify");
    assert_eq!(work.idle, 0, "a mutating tool call resets the fuse");
    work.verdict(true, Decision::GoalContinue("more".to_string()))
        .expect("continue");
    work.verify(Snapshot::default()).expect("verify");
    assert_eq!(work.idle, 1, "the flag is per turn");
}

#[test]
fn a_long_plan_gets_a_compact_ledger() {
    let plan: String = std::iter::once("## Steps\n\n### Big\n\n".to_string())
        .chain((1..=45).map(|n| format!("{n}. Step {n}.\n")))
        .collect();
    let mut work = Work::new(&plan, Policy::Steps);
    work.select(&plan, &[]).expect("select");
    work.execute(Snapshot::default()).expect("execute");
    pass(&mut work);
    pass(&mut work);
    let ledger = work.progress().ledger;
    assert_eq!(
        ledger,
        vec!["I. Big", "  [x] 2 done", "  [/] Step 3.", "  [ ] 42 later"],
        "{ledger:?}"
    );
}

#[test]
fn a_deferral_keeps_the_turn_open_three_times_then_stops() {
    let mut work = Work::new("build it", Policy::Goal { max_turns: 9 });
    work.select("build it", &[]).expect("select");
    work.execute(Snapshot::default()).expect("execute");
    for _ in 0..3 {
        assert!(work.defer("job still running".to_string()).expect("defer"));
        assert_eq!(work.stage(), Some(Stage::Execute));
    }
    assert!(!work.defer("job still running".to_string()).expect("capped"));
    assert_eq!(work.pending.len(), 3);
    assert_eq!(work.progress().rounds, 0, "deferrals are not turns");
}

#[test]
fn a_failing_gate_with_no_change_ends_as_stalled_not_three_strikes() {
    let mut work = Work::new("build it", Policy::Goal { max_turns: 9 });
    work.select("build it", &["false".to_string()])
        .expect("select");
    work.execute(Snapshot::default()).expect("execute");
    for _ in 0..3 {
        work.verify(Snapshot::default()).expect("verify");
        if work.terminal() {
            break;
        }
        work.verdict(false, Decision::Automatic).expect("fail");
    }
    assert!(
        matches!(work.status, Status::Abandoned(ref reason) if reason.starts_with("stalled:")),
        "{:?}",
        work.status
    );
    assert_eq!(work.progress().exit_code(), Some(3));
}
