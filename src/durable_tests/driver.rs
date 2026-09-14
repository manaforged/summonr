use std::path::{Path, PathBuf};

use crate::child::{ChildRecord, ChildState};
use crate::durable::driver::{DriverError, Exposure, SessionDriver, active_at, work_at};
use crate::durable::id::SessionId;
use crate::durable::log::{self, Line, Usage};
use crate::durable::work::{Decision, Policy, Stage, Work, WorkPhase};
use crate::provider::{Image, Role, Source, ToolCall, ToolFunction, ToolKind, UserInput};

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
    driver.start_input(&text.into()).expect("start");
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
fn fresh_child_has_lineage_without_parent_history_or_work() {
    let (home, workspace) = home();
    let parent = open(home.path(), &workspace);
    let child = SessionDriver::open_child(
        home.path(),
        &workspace,
        "child-model",
        "child-system",
        parent.id(),
    )
    .expect("open child");

    assert!(child.replay().history.is_empty());
    assert_eq!(child.depth(), 1);
    assert!(matches!(child.work_progress(), Err(DriverError::NoWork)));
    let header = log::header(child.path()).expect("child header");
    assert!(matches!(
        header,
        Line::Session {
            parent: Some(id),
            ..
        } if id == parent.id()
    ));
}

#[test]
fn resume_interrupts_a_running_child_record() {
    let (home, workspace) = home();
    let mut parent = open(home.path(), &workspace);
    let path = parent.path().to_path_buf();
    let child = SessionDriver::open_child(
        home.path(),
        &workspace,
        "child-model",
        "child-system",
        parent.id(),
    )
    .expect("open child");
    parent
        .start_child(ChildRecord {
            version: 1,
            id: child.id(),
            parent: parent.id(),
            assignment: "inspect".to_string(),
            model: "child-model".to_string(),
            workspace: workspace.canonicalize().expect("canonical workspace"),
            depth: 1,
            access: crate::child::Access::Implement,
            call: None,
            state: ChildState::Running,
        })
        .expect("record child");
    drop(parent);

    let resumed = resume(home.path(), &path, &workspace);
    assert!(matches!(
        resumed.children().as_slice(),
        [ChildRecord {
            state: ChildState::Interrupted(_),
            ..
        }]
    ));
}

#[test]
fn parent_accepts_more_than_four_concurrent_children() {
    let (home, workspace) = home();
    let mut parent = open(home.path(), &workspace);
    let workspace = workspace.canonicalize().expect("canonical workspace");
    let mut records = Vec::new();
    for index in 0..5 {
        let record = ChildRecord {
            version: 1,
            id: SessionId::from_u128(index as u128 + 1),
            parent: parent.id(),
            assignment: format!("inspect {index}"),
            model: "child-model".to_string(),
            workspace: workspace.clone(),
            depth: 1,
            access: crate::child::Access::Research,
            call: None,
            state: ChildState::Running,
        };
        parent.start_child(record.clone()).expect("start child");
        records.push(record);
    }
    assert_eq!(parent.children().len(), 5);
    for record in records {
        parent
            .finish_child(record.interrupt("done"))
            .expect("finish child");
    }
}

#[test]
fn parent_retains_completed_children_without_a_lifetime_quota() {
    let (home, workspace) = home();
    let mut parent = open(home.path(), &workspace);
    for index in 0..65 {
        let record = ChildRecord {
            version: 1,
            id: SessionId::from_u128(index as u128 + 1),
            parent: parent.id(),
            assignment: "inspect".to_string(),
            model: "child-model".to_string(),
            workspace: workspace.canonicalize().expect("canonical workspace"),
            depth: 1,
            access: crate::child::Access::Implement,
            call: None,
            state: ChildState::Running,
        };
        parent.start_child(record.clone()).expect("start child");
        parent
            .finish_child(record.interrupt("done"))
            .expect("finish child");
    }
    let overflow = ChildRecord {
        version: 1,
        id: SessionId::from_u128(u128::MAX),
        parent: parent.id(),
        assignment: "overflow".to_string(),
        model: "child-model".to_string(),
        workspace: workspace.canonicalize().expect("canonical workspace"),
        depth: 1,
        access: crate::child::Access::Implement,
        call: None,
        state: ChildState::Running,
    };

    parent.start_child(overflow.clone()).expect("start child");
    parent
        .finish_child(overflow.interrupt("done"))
        .expect("finish child");
    assert_eq!(parent.children().len(), 66);
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
                ToolCall {
                    continuation: None,
                    id: "c1".into(),
                    kind: ToolKind::Function,
                    function: ToolFunction {
                        name: "read_file".into(),
                        arguments: r#"{"path":"a"}"#.into(),
                    },
                },
                ToolCall {
                    continuation: None,
                    id: "c2".into(),
                    kind: ToolKind::Function,
                    function: ToolFunction {
                        name: "read_file".into(),
                        arguments: r#"{"path":"b"}"#.into(),
                    },
                },
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
    driver
        .start_input(&"change the source".into())
        .expect("start");
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
    driver.start_input(&"write".into()).expect("start");
    assert_eq!(driver.stage(), Some(Stage::Execute));
    answer(&mut driver, "done");
    std::fs::write(workspace.join("new.txt"), b"x").expect("edit");
    let paths = driver.enter_verify().expect("capture");
    assert_eq!(paths, vec!["new.txt".to_string()]);
    let verdict = driver.prepare_verdict(true).expect("verdict");
    assert!(verdict.passed && verdict.diagnostic.is_none());
    driver
        .commit_verdict(verdict, Decision::Automatic, Some("Turn receipt"))
        .expect("commit");
    assert!(!driver.busy());
    assert_eq!(driver.replay().history[1].content, "done");
    assert_eq!(driver.replay().history[2].content, "Turn receipt");
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
fn changes_outside_the_candidate_paths_reject_verification() {
    for action in ["add", "change", "delete"] {
        let (home, workspace) = home();
        let mut driver = open(home.path(), &workspace);
        start(&mut driver, "write");
        answer(&mut driver, "done");
        assert!(driver.enter_verify().expect("capture").is_empty());
        match action {
            "add" => {
                std::fs::write(workspace.join("new.rs"), b"generated source").expect("add source")
            }
            "change" => {
                std::fs::write(workspace.join("src.txt"), b"new contents").expect("change source")
            }
            _ => std::fs::remove_file(workspace.join("src.txt")).expect("delete source"),
        }
        let verdict = driver.prepare_verdict(true).expect("verdict");
        assert!(!verdict.passed, "{action} must invalidate verification");
        assert_eq!(
            verdict.diagnostic.as_deref(),
            Some("workspace changed after candidate capture")
        );
    }
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
fn ignored_build_output_does_not_change_the_candidate() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    start(&mut driver, "check");
    answer(&mut driver, "done");
    driver.enter_verify().expect("capture");
    std::fs::create_dir(workspace.join("target")).expect("build directory");
    std::fs::write(workspace.join("target/result"), "built").expect("build output");
    let verdict = driver.prepare_verdict(true).expect("verdict");
    assert!(
        verdict.passed,
        "excluded build output changed the candidate"
    );
    assert_eq!(verdict.diagnostic, None);
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
        .fail_provider(attempt, "denied", 25, true)
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
    assert!(matches!(
        driver.steer(&"early".into()),
        Err(DriverError::NoTurn)
    ));
    start(&mut driver, "implement");
    let image = Image::encode("image/png", b"\x89PNG\r\n\x1a\n");
    let input = UserInput {
        text: "prefer the short path\n\n--- route.txt ---\nvia north\n".to_string(),
        images: vec![image.clone()],
        source: Some(Source {
            text: "prefer the short path @route.txt".to_string(),
            images: 0,
        }),
    };
    driver.steer(&input).expect("steer");
    assert!(matches!(
        driver.steer(&"   ".into()),
        Err(DriverError::Empty)
    ));
    assert!(matches!(
        driver.start_input(&"again".into()),
        Err(DriverError::Busy)
    ));
    drop(driver);

    let resumed = resume(home.path(), &path, &workspace);
    let history = resumed.replay().history;
    assert_eq!(history[0].content, "implement");
    assert_eq!(history[1].content, input.text);
    assert_eq!(history[1].images, [image]);
    assert!(log::read(&path).expect("journal").iter().any(|line| matches!(line, Line::User { source: Some(text), start: false, .. } if text == "prefer the short path @route.txt")));
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
    driver
        .abort("nothing open", None)
        .expect("idle abort is fine");
    start(&mut driver, "read");
    driver
        .start_provider("test-model", exposure())
        .expect("provider");
    driver
        .abort("cancelled by user", Some("Turn receipt"))
        .expect("abort");
    assert!(!driver.busy());
    let log = std::fs::read_to_string(driver.path()).expect("log");
    assert!(log.contains("cancelled by user"));
    assert!(log.contains(r#""kind":"receipt""#));
    assert_eq!(driver.replay().spent_tokens, 100);
    driver.abort("already aborted", None).expect("repeat abort");
    assert_eq!(driver.replay().spent_tokens, 100);
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
    let child_path = driver.fork(None).expect("fork");
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
fn earlier_branch_preserves_the_closed_turn_and_parent_bytes() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    let image = Image::encode("image/png", b"saved image bytes");
    driver.prepare_turn_work("first", &[]).expect("work");
    driver
        .start_input(&UserInput {
            text: "first".to_string(),
            images: vec![image.clone()],
            source: None,
        })
        .expect("first input");
    let attempt = driver
        .start_provider("test-model", exposure())
        .expect("provider");
    let tool = ToolCall {
        id: "read-first".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: "read_file".to_string(),
            arguments: r#"{"path":"src.txt"}"#.to_string(),
        },
        continuation: None,
    };
    let calls = driver
        .finish_provider_with_tools(attempt, "", "", usage(10, 5, 100), &[tool])
        .expect("tool call");
    driver
        .finish_tools(&[(calls[0], "hello", true, 1)])
        .expect("tool result");
    answer(&mut driver, "first done");
    driver.close().expect("close first");
    let prefix = log::read(driver.path()).expect("first journal");
    let expected = driver.replay().history;
    driver.clear().expect("clear after first");
    start(&mut driver, "later");
    answer(&mut driver, "later done");
    driver.close().expect("close later");
    let parent = std::fs::read(driver.path()).expect("parent bytes");
    let child_path = driver.fork(Some(0)).expect("earlier branch");
    let child_lines = log::read(&child_path).expect("child journal");
    assert_eq!(&child_lines[1..], &prefix[1..]);
    assert_eq!(
        std::fs::read(driver.path()).expect("unchanged parent"),
        parent
    );
    assert_eq!(
        std::fs::read(workspace.join("src.txt")).expect("workspace"),
        b"hello"
    );
    let child = resume(home.path(), &child_path, &workspace);
    assert_eq!(child.replay().history, expected);
    assert_eq!(child.replay().history[0].images, [image]);
    assert!(matches!(child.work_progress(), Err(DriverError::NoWork)));
    assert!(
        matches!(log::header(&child_path).expect("header"), Line::Session { parent: Some(id), .. } if id == driver.id())
    );
    driver.fork(Some(9)).expect_err("missing turn");
    start(&mut driver, "still active");
    assert!(matches!(driver.fork(Some(0)), Err(DriverError::Busy)));
    assert!(matches!(driver.fork(None), Err(DriverError::Busy)));
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
    assert_eq!(
        active_at(&path, &workspace),
        Some((Policy::Steps, plan.to_string())),
        "an unfinished job is reported whatever the caller asks for"
    );
    assert_eq!(active_at(&path, home.path()), None);
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
        driver.start_input(&"hi".into()),
        Err(DriverError::NoWork)
    ));
    assert!(matches!(
        driver.start_input(&"  ".into()),
        Err(DriverError::Empty)
    ));
    assert!(driver.accept_plan().is_err());

    driver.prepare_plan_work("plan it").expect("plan work");
    driver.start_input(&"plan it".into()).expect("start");
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
            "# Plan\n\n1. Do it\n\nTurn receipt",
        )
        .expect("finish plan");
    assert!(!driver.busy());
    driver
        .prepare_turn_work("implement", &["true".to_string()])
        .expect("the next implementation turn accepts the plan");
    assert_eq!(
        driver.work_progress().expect("progress").phase,
        WorkPhase::Execute
    );
    assert!(driver.accept_plan().is_err(), "nothing left to accept");
    driver
        .prepare_turn_work("implement", &[])
        .expect("changed checks are re-selected, not refused");
    assert!(
        driver.completion_checks().expect("checks").is_empty(),
        "the new check list is live"
    );
}

#[test]
fn a_plan_turn_that_selected_nothing_does_not_trap_the_session() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    driver.prepare_plan_work("plan it").expect("plan work");
    assert_eq!(
        driver.work_progress().expect("progress").phase,
        WorkPhase::Plan
    );
    driver
        .prepare_turn_work("implement", &[])
        .expect("a fresh Work replaces the empty plan");
    assert_eq!(
        driver.work_progress().expect("progress").phase,
        WorkPhase::Execute
    );
    assert_eq!(driver.objective().expect("objective"), "implement");
}

#[test]
fn queued_continuations_land_before_the_next_input() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    driver
        .prepare_turn_work("fix", &["false".to_string()])
        .expect("work");
    driver.start_input(&"fix".into()).expect("start");
    answer(&mut driver, "tried");
    driver.enter_verify().expect("capture");
    let verdict = driver.prepare_verdict(false).expect("verdict");
    driver
        .commit_verdict(verdict, Decision::Automatic, None)
        .expect("commit");

    driver
        .prepare_turn_work("again", &["false".to_string()])
        .expect("same work");
    let queued = driver.start_input(&"again".into()).expect("start");
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

#[test]
fn a_goal_matches_its_session_whatever_the_turn_limit() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    let path = driver.path().to_path_buf();
    driver
        .prepare_external_work("finish it", Policy::Goal { max_turns: 20 }, &[])
        .expect("select goal");
    assert!(work_at(
        &path,
        &workspace,
        Policy::Goal { max_turns: 7 },
        "finish it"
    ));
    assert!(!work_at(&path, &workspace, Policy::Steps, "finish it"));
    driver
        .prepare_external_work("finish it", Policy::Goal { max_turns: 7 }, &[])
        .expect("a rerun with another turn limit attaches to the same goal");
    assert_eq!(driver.work_progress().expect("progress").max_turns, 20);
}

#[test]
fn resume_into_a_deleted_workspace_names_the_path() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    let id = driver.id();
    driver.prepare_turn_work("keep going", &[]).expect("work");
    drop(driver);
    std::fs::remove_dir_all(&workspace).expect("remove workspace");
    let error = SessionDriver::resume(home.path(), id, &workspace)
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(error.contains("no longer exists"), "{error}");
    assert!(error.contains(&workspace.display().to_string()), "{error}");
}

#[test]
fn a_resumed_interrupted_turn_tells_the_model_it_was_interrupted() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    let path = driver.path().to_path_buf();
    driver.prepare_turn_work("do it", &[]).expect("work");
    driver.start_input(&"do it".into()).expect("start");
    let _attempt = driver
        .start_provider("test-model", exposure())
        .expect("provider");
    drop(driver);
    let mut resumed = resume(home.path(), &path, &workspace);
    resumed.prepare_turn_work("do it", &[]).expect("work again");
    let queued = resumed.start_input(&"do it".into()).expect("start again");
    assert!(
        queued.iter().any(|text| text.contains("was interrupted")),
        "{queued:?}"
    );
}

#[test]
fn a_gate_change_requires_recapture_before_the_next_base() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    driver
        .prepare_external_work("keep going", Policy::Goal { max_turns: 5 }, &[])
        .expect("goal");
    driver.start_input(&"go".into()).expect("start");
    std::fs::write(workspace.join("edit.txt"), "model\n").expect("model edit");
    let paths = driver.enter_verify().expect("capture");
    assert_eq!(paths, vec!["edit.txt"]);
    std::fs::write(workspace.join("stamp.txt"), "gate\n").expect("gate write");
    let verdict = driver.prepare_verdict(true).expect("verdict");
    assert!(!verdict.passed, "the gate changed an uncaptured file");
    driver
        .commit_verdict(verdict, Decision::Automatic, None)
        .expect("reject");
    driver.start_input(&"again".into()).expect("start");
    let paths = driver.enter_verify().expect("capture");
    assert_eq!(paths, vec!["stamp.txt"]);
    let verdict = driver.prepare_verdict(true).expect("rechecked verdict");
    assert!(verdict.passed, "the new candidate is stable");
}

#[test]
fn a_finished_session_resumes_from_anywhere_after_its_tree_is_gone() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    let path = driver.path().to_path_buf();
    driver.prepare_turn_work("ship it", &[]).expect("work");
    driver.start_input(&"ship it".into()).expect("start");
    driver.enter_verify().expect("capture");
    let verdict = driver.prepare_verdict(true).expect("verdict");
    driver
        .commit_verdict(verdict, Decision::Automatic, None)
        .expect("complete");
    drop(driver);
    std::fs::remove_dir_all(&workspace).expect("remove");
    let elsewhere = home.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).expect("elsewhere");
    let resumed = resume(home.path(), &path, &elsewhere);
    assert_eq!(
        resumed.workspace().canonicalize().ok(),
        elsewhere.canonicalize().ok(),
        "a terminal session follows the caller"
    );
}

#[test]
fn abort_answers_a_completed_child_with_its_result() {
    let (home, workspace) = home();
    let mut driver = open(home.path(), &workspace);
    start(&mut driver, "audit");
    let attempt = driver
        .start_provider("test-model", exposure())
        .expect("provider");
    driver
        .finish_provider_with_tools(
            attempt,
            "",
            "",
            usage(1, 1, 1),
            &["t1", "t2"].map(|id| ToolCall {
                continuation: None,
                id: id.into(),
                kind: ToolKind::Function,
                function: ToolFunction {
                    name: "task".into(),
                    arguments: r#"{"assignment":"inspect","access":"research"}"#.into(),
                },
            }),
        )
        .expect("plan tools");
    let mut record = ChildRecord {
        version: 1,
        id: SessionId::from_u128(7),
        parent: driver.id(),
        assignment: "inspect".to_string(),
        model: "child-model".to_string(),
        workspace: workspace.canonicalize().expect("canonical workspace"),
        depth: 1,
        access: crate::child::Access::Research,
        call: Some("t1".to_string()),
        state: ChildState::Running,
    };
    driver.start_child(record.clone()).expect("start child");
    record.state = ChildState::Completed(crate::child::ChildResult {
        text: "the report".to_string(),
        changed_paths: Vec::new(),
        changed_paths_total: 0,
        rounds: 3,
        usage: crate::provider::Usage::default(),
        billed_tokens: 0,
        elapsed_millis: 5,
        diagnostic: None,
    });
    driver.finish_child(record).expect("finish child");
    driver.abort("cancelled by user", None).expect("abort");

    let history = driver.replay().history;
    assert_eq!(history.len(), 4, "user, assistant, completed, aborted");
    assert_eq!(history[2].tool_call_id.as_deref(), Some("t1"));
    assert!(
        history[2].content.contains("the report"),
        "{}",
        history[2].content
    );
    assert!(
        history[2].content.contains("completed"),
        "{}",
        history[2].content
    );
    assert_eq!(history[3].tool_call_id.as_deref(), Some("t2"));
    assert_eq!(history[3].content, "aborted");
}
