use std::path::{Path, PathBuf};

use crate::durable::driver::{DriverError, Exposure, SessionDriver, work_at};
use crate::durable::id::SessionId;
use crate::durable::log::{self, Line, Usage};
use crate::durable::work::{Decision, Policy, Stage, Work};
use crate::provider::Role;

fn home() -> (tempfile::TempDir, PathBuf) {
    let home = tempfile::tempdir().expect("tempdir");
    let workspace = home.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace");
    std::fs::write(workspace.join("src.txt"), b"hello").expect("file");
    (home, workspace)
}

fn open(home: &Path, workspace: &Path) -> SessionDriver {
    SessionDriver::open(home, workspace, "test-model", "You are Mage.").expect("open")
}

fn resume(home: &Path, path: &Path, workspace: &Path) -> SessionDriver {
    let id = log::parse_path(path).expect("session path");
    SessionDriver::resume(home, id, workspace).expect("resume")
}

fn exposure() -> Exposure {
    Exposure {
        max_input_tokens: 70,
        max_output_tokens: 30,
    }
}

fn usage(input: u64, output: u64, millis: u64) -> Usage {
    Usage {
        input,
        output,
        cached: 0,
        millis,
    }
}

fn start(driver: &mut SessionDriver, text: &str) {
    driver.prepare_turn_work(text, &[]).expect("work");
    driver.start_input(text, &[]).expect("start");
}

fn answer(driver: &mut SessionDriver, text: &str) {
    let attempt = driver
        .start_provider("test-model", exposure())
        .expect("provider");
    driver
        .finish_provider(attempt, text, "", usage(10, 5, 100))
        .expect("finish");
}

fn kinds(path: &Path) -> Vec<String> {
    log::read(path)
        .expect("read")
        .iter()
        .map(|line| {
            serde_json::to_value(line).expect("encode")["kind"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect()
}

#[test]
fn open_round_and_resume_continues() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    let path = driver.path().to_path_buf();
    assert!(path.starts_with(home.path().join("sessions")));
    assert_eq!(log::session_path(home.path(), driver.id()), path);

    start(&mut driver, "build the cutover");
    answer(&mut driver, "done");
    driver.close().expect("close");
    drop(driver);

    let mut resumed = resume(home.path(), &path, &workspace);
    assert!(!resumed.busy());
    start(&mut resumed, "second turn");
    answer(&mut resumed, "again");
    let replay = resumed.replay();
    let texts: Vec<(Role, &str)> = replay
        .history
        .iter()
        .map(|message| (message.role, message.content.as_str()))
        .collect();
    assert_eq!(
        texts,
        [
            (Role::User, "build the cutover"),
            (Role::Assistant, "done"),
            (Role::User, "second turn"),
            (Role::Assistant, "again"),
        ]
    );
    assert_eq!(replay.spent_tokens, 30);
    assert_eq!(replay.elapsed_millis, 200);
}

#[test]
fn resume_missing_session_fails() {
    let (home, workspace) = home();
    assert!(SessionDriver::resume(home.path(), SessionId::from_u128(99), &workspace).is_err());
}

#[test]
fn resume_aborts_the_open_turn_and_answers_its_tool_calls() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    let path = driver.path().to_path_buf();
    start(&mut driver, "killed mid-turn");
    let attempt = driver
        .start_provider("test-model", exposure())
        .expect("provider");
    let calls = driver
        .finish_provider_with_tools(
            attempt,
            "",
            "",
            usage(1, 1, 1),
            &[
                ("read_file", r#"{"path":"a"}"#, "c1"),
                ("read_file", r#"{"path":"b"}"#, "c2"),
            ],
        )
        .expect("plan tools");
    driver
        .finish_tools(&[(calls[0], "A", true, 3)])
        .expect("first result");
    drop(driver);

    let mut resumed = resume(home.path(), &path, &workspace);
    assert!(!resumed.busy(), "resume settles the dead process's turn");
    let history = resumed.replay().history;
    assert_eq!(history.len(), 4, "user, assistant, answered, aborted");
    assert_eq!(history[3].tool_call_id.as_deref(), Some("c2"));
    assert_eq!(history[3].content, "aborted");
    assert_eq!(kinds(&path).last().map(String::as_str), Some("close"));
    start(&mut resumed, "next turn");
}

#[test]
fn resume_returns_an_interrupted_verify_to_execute() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    let path = driver.path().to_path_buf();
    driver
        .prepare_turn_work("change the source", &["true".to_string()])
        .expect("work");
    driver.start_input("change the source", &[]).expect("start");
    answer(&mut driver, "done");
    std::fs::write(workspace.join("src.txt"), b"changed").expect("edit");
    let paths = driver.enter_verify().expect("capture");
    assert_eq!(paths, vec!["src.txt".to_string()]);
    drop(driver);

    let resumed = resume(home.path(), &path, &workspace);
    let work = Work::load(&log::work_path(&path))
        .expect("load")
        .expect("work");
    assert_eq!(work.stage(), Some(Stage::Execute));
    assert!(work.candidate.is_none());
    assert!(!resumed.busy());
}

#[test]
fn verdict_writes_the_receipt_and_closes_the_turn() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    driver
        .prepare_turn_work("write", &["true".to_string()])
        .expect("work");
    driver.start_input("write", &[]).expect("start");
    assert_eq!(driver.stage(), Some(Stage::Execute));
    answer(&mut driver, "done");
    std::fs::write(workspace.join("new.txt"), b"x").expect("edit");
    let paths = driver.enter_verify().expect("capture");
    assert_eq!(paths, vec!["new.txt".to_string()]);
    let verdict = driver.prepare_verdict(true).expect("verdict");
    assert!(verdict.passed && verdict.diagnostic.is_none());
    driver
        .commit_verdict(verdict, Decision::Automatic, Some("done\n\nTurn receipt"))
        .expect("commit");
    assert!(!driver.busy());
    assert_eq!(driver.replay().history[1].content, "done\n\nTurn receipt");
    assert_eq!(
        driver.work_progress().expect("progress").phase,
        crate::WorkPhase::Completed
    );
    assert_eq!(
        kinds(driver.path()),
        ["session", "user", "assistant", "receipt", "close"]
    );
}

#[test]
fn untouched_file_churn_during_verify_keeps_the_verdict() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    start(&mut driver, "write");
    answer(&mut driver, "done");
    driver.enter_verify().expect("capture");
    std::fs::write(workspace.join("Cargo.lock"), b"regenerated").expect("gate write");
    let verdict = driver.prepare_verdict(true).expect("verdict");
    assert!(verdict.passed, "untouched churn must not fail the verdict");
    assert!(verdict.diagnostic.is_none());
}

#[test]
fn a_changed_candidate_file_during_verify_fails_the_verdict() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    start(&mut driver, "write");
    std::fs::write(workspace.join("out.txt"), b"first").expect("edit");
    answer(&mut driver, "done");
    driver.enter_verify().expect("capture");
    std::fs::write(workspace.join("out.txt"), b"second-and-longer").expect("re-edit");
    let verdict = driver.prepare_verdict(true).expect("verdict");
    assert!(!verdict.passed);
    assert_eq!(
        verdict.diagnostic.as_deref(),
        Some("workspace changed after candidate capture")
    );
}

#[test]
fn failed_provider_charges_exposure_without_adding_history() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    start(&mut driver, "read the spec");
    let attempt = driver
        .start_provider("test-model", exposure())
        .expect("provider");
    driver
        .fail_provider(attempt, "denied", 25)
        .expect("failure settles");
    let replay = driver.replay();
    assert_eq!(replay.spent_tokens, 100);
    assert_eq!(replay.elapsed_millis, 25);
    assert_eq!(replay.history.len(), 1);
    assert!(matches!(
        driver.finish_provider(attempt, "late", "", usage(1, 1, 1)),
        Err(DriverError::NoProvider)
    ));
}

#[test]
fn summary_and_judge_rounds_charge_without_messages() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    start(&mut driver, "go");
    let attempt = driver
        .start_judge("judge-model", exposure())
        .expect("judge");
    driver
        .finish_provider(attempt, r#"{"done":true}"#, "", usage(4, 1, 9))
        .expect("finish judge");
    let attempt = driver
        .start_summary("summary-model", exposure())
        .expect("summary");
    driver
        .finish_provider(attempt, "SUMMARY", "", usage(2, 2, 2))
        .expect("finish summary");
    let replay = driver.replay();
    assert_eq!(replay.spent_tokens, 9);
    assert_eq!(replay.history.len(), 1, "neither round is a message");
    let log = std::fs::read_to_string(driver.path()).expect("log");
    assert!(log.contains("\"note\":\"goal judge\""));
}

#[test]
fn steer_survives_resume_and_needs_an_open_turn() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    let path = driver.path().to_path_buf();
    assert!(matches!(driver.steer("early"), Err(DriverError::NoTurn)));
    start(&mut driver, "implement");
    driver.steer("prefer the short path").expect("steer");
    assert!(matches!(driver.steer("   "), Err(DriverError::Empty)));
    assert!(matches!(
        driver.start_input("again", &[]),
        Err(DriverError::Busy)
    ));
    drop(driver);

    let resumed = resume(home.path(), &path, &workspace);
    let history = resumed.replay().history;
    assert_eq!(history[0].content, "implement");
    assert_eq!(history[1].content, "prefer the short path");
}

#[test]
fn compact_rebases_on_the_open_turn_and_clear_needs_idle() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    let path = driver.path().to_path_buf();
    start(&mut driver, "implement");
    assert!(matches!(driver.clear(), Err(DriverError::Busy)));
    answer(&mut driver, "long answer");
    driver
        .compact("Keep the short path.", &[])
        .expect("compact");
    driver.close().expect("close");
    drop(driver);

    let mut resumed = resume(home.path(), &path, &workspace);
    let history = resumed.replay().history;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].content, "implement");
    assert_eq!(history[1].role, Role::System);
    resumed.clear().expect("clear");
    assert!(resumed.replay().history.is_empty());
}

#[test]
fn abort_is_idempotent_and_records_its_reason() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    driver.abort("nothing open").expect("idle abort is fine");
    start(&mut driver, "read");
    driver
        .start_provider("test-model", exposure())
        .expect("provider");
    driver.abort("cancelled by user").expect("abort");
    assert!(!driver.busy());
    let log = std::fs::read_to_string(driver.path()).expect("log");
    assert!(log.contains("cancelled by user"));
    assert_eq!(driver.replay().spent_tokens, 0);
    start(&mut driver, "next");
}

#[test]
fn budget_persists_and_only_changes_are_written() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    let path = driver.path().to_path_buf();
    driver.set_budget(Some(1_000), None).expect("budget");
    driver.set_budget(Some(1_000), None).expect("same budget");
    assert_eq!(kinds(&path), ["session", "budget"]);
    drop(driver);
    let resumed = resume(home.path(), &path, &workspace);
    assert_eq!(resumed.budget_limits(), (Some(1_000), None));
}

#[test]
fn fork_copies_the_conversation_but_not_the_work() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    start(&mut driver, "first");
    answer(&mut driver, "done");
    driver.close().expect("close");
    let child_path = driver.fork().expect("fork");
    assert_ne!(child_path, driver.path());
    let child = resume(home.path(), &child_path, &workspace);
    assert_ne!(child.id(), driver.id());
    assert_eq!(child.replay().history, driver.replay().history);
    assert!(matches!(child.work_progress(), Err(DriverError::NoWork)));
    assert!(matches!(
        log::header(&child_path).expect("header"),
        Line::Session { parent, .. } if parent == Some(driver.id())
    ));
}

#[test]
fn external_work_resumes_only_the_same_plan_and_checks() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    let path = driver.path().to_path_buf();
    let plan = "## Steps\n\n1. Implement the plan.";
    assert!(
        driver
            .prepare_external_work(plan, Policy::Single, &[])
            .is_err()
    );
    driver
        .prepare_external_work(plan, Policy::Steps, &["true".to_string()])
        .expect("select");
    assert_eq!(driver.work_progress().expect("progress").current, Some(1));
    assert!(work_at(&path, &workspace, Policy::Steps, plan));
    assert!(!work_at(&path, &workspace, Policy::Steps, "other plan"));
    assert!(!work_at(
        &path,
        &workspace,
        Policy::Goal { max_turns: 1 },
        plan
    ));
    assert!(
        !work_at(&path, home.path(), Policy::Steps, plan),
        "another workspace"
    );

    assert!(
        driver
            .prepare_external_work(plan, Policy::Steps, &["false".to_string()])
            .is_err(),
        "changed checks are refused"
    );
    assert!(
        driver
            .prepare_external_work(
                "## Steps\n\n1. Other.",
                Policy::Steps,
                &["true".to_string()]
            )
            .is_err(),
        "another objective is refused"
    );
    drop(driver);

    let mut resumed = resume(home.path(), &path, &workspace);
    resumed
        .prepare_external_work(plan, Policy::Steps, &["true".to_string()])
        .expect("same plan resumes");
    assert_eq!(resumed.work_progress().expect("progress").current, Some(1));
}

#[test]
fn a_turn_needs_work_and_a_plan_must_be_accepted_first() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    assert!(matches!(
        driver.start_input("hi", &[]),
        Err(DriverError::NoWork)
    ));
    assert!(matches!(
        driver.start_input("  ", &[]),
        Err(DriverError::Empty)
    ));
    assert!(driver.accept_plan().is_err());

    driver.prepare_plan_work("plan it").expect("plan work");
    driver.start_input("plan it", &[]).expect("start");
    let attempt = driver
        .start_provider("test-model", exposure())
        .expect("provider");
    driver
        .finish_plan(
            attempt,
            "# Plan\n\n1. Do it",
            "",
            usage(1, 1, 1),
            &["true".to_string()],
        )
        .expect("finish plan");
    assert!(!driver.busy());
    assert!(
        driver
            .prepare_turn_work("implement", &["true".to_string()])
            .is_err(),
        "the plan must be accepted before implementation"
    );
    driver.accept_plan().expect("accept");
    driver
        .prepare_turn_work("implement", &["true".to_string()])
        .expect("execute attaches");
    assert!(
        driver.prepare_turn_work("implement", &[]).is_err(),
        "changed checks are refused"
    );
}

#[test]
fn queued_continuations_land_before_the_next_input() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    driver
        .prepare_turn_work("fix", &["false".to_string()])
        .expect("work");
    driver.start_input("fix", &[]).expect("start");
    answer(&mut driver, "tried");
    driver.enter_verify().expect("capture");
    let verdict = driver.prepare_verdict(false).expect("verdict");
    driver
        .commit_verdict(verdict, Decision::Automatic, None)
        .expect("commit");

    driver
        .prepare_turn_work("again", &["false".to_string()])
        .expect("same work");
    let queued = driver.start_input("again", &[]).expect("start");
    assert_eq!(
        queued,
        vec!["Repair the failed completion check.".to_string()]
    );
    let history = driver.replay().history;
    assert_eq!(history[2].content, "Repair the failed completion check.");
    assert_eq!(history[3].content, "again");
}

#[test]
fn resume_uses_the_recorded_workspace_not_the_caller() {
    let (home, workspace) = home();
    let driver = open(home.path(), &workspace);
    let path = driver.path().to_path_buf();
    drop(driver);

    let elsewhere = home.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).expect("elsewhere");
    let resumed = resume(home.path(), &path, &elsewhere);

    let recorded = std::fs::canonicalize(&workspace).expect("canonical");
    assert_eq!(
        resumed.workspace(),
        recorded,
        "resume must use the session's recorded workspace, not the caller's cwd"
    );
}
