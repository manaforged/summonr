//! Product constructor tests for format-2 open and resume.

use crate::durable::effect::Usage;
use crate::durable::id::SessionId;
use crate::durable::journal;
use crate::durable::product::{self, Open};
use crate::provider::Role;

fn usage() -> Usage {
    Usage {
        input_tokens: Some(10),
        output_tokens: Some(5),
        cached_input_tokens: Some(0),
        live_millis: Some(100),
    }
}

fn workspace_home() -> (tempfile::TempDir, std::path::PathBuf) {
    let home = tempfile::tempdir().expect("temporary home must exist");
    let workspace = home.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace must be created");
    std::fs::write(workspace.join("src.txt"), b"hello").expect("workspace file must be written");
    (home, workspace)
}

fn open_spec(home: &std::path::Path, workspace: &std::path::Path) -> Open {
    Open {
        home: home.to_path_buf(),
        workspace: workspace.to_path_buf(),
        model: "test-model".to_string(),
        system: "You are Mage.".to_string(),
        endpoint: "https://example.test/v1".to_string(),
        adapter: "openai-compatible".to_string(),
    }
}

#[test]
fn open_round_and_resume_continues() {
    let (home, workspace) = workspace_home();
    let mut driver = product::open(&open_spec(home.path(), &workspace)).expect("open must succeed");

    let path = driver.path().to_path_buf();
    assert!(
        path.starts_with(home.path().join("sessions-v2")),
        "journal must live under home/sessions-v2"
    );
    let session = journal::parse_path(&path).expect("journal path must parse");
    assert_eq!(
        journal::session_path(home.path(), session),
        path,
        "session_path must round-trip the parsed id"
    );

    driver
        .start_operation("build the cutover")
        .expect("operation must open");
    let attempt = driver
        .start_provider(br#"{"model":"test-model"}"#)
        .expect("provider must start");
    driver
        .finish_provider(attempt, "done", "", usage())
        .expect("provider must finish");
    driver.close_operation().expect("operation must close");
    drop(driver);

    let mut driver =
        product::resume(home.path(), session, &workspace).expect("resume must succeed");
    driver
        .start_operation("second turn")
        .expect("second operation must open");
    let attempt = driver
        .start_provider(br#"{"model":"test-model"}"#)
        .expect("second provider must start");
    driver
        .finish_provider(attempt, "again", "", usage())
        .expect("second provider must finish");

    let replay = driver.replay();
    assert_eq!(replay.history.len(), 4, "two turns project four messages");
    assert_eq!(replay.history[0].role, Role::User);
    assert_eq!(replay.history[0].content, "build the cutover");
    assert_eq!(replay.history[1].role, Role::Assistant);
    assert_eq!(replay.history[1].content, "done");
    assert_eq!(replay.history[2].role, Role::User);
    assert_eq!(replay.history[2].content, "second turn");
    assert_eq!(replay.history[3].role, Role::Assistant);
    assert_eq!(replay.history[3].content, "again");
    drop(driver);
}

#[test]
fn resume_missing_file_fails() {
    let (home, workspace) = workspace_home();
    let error = product::resume(home.path(), SessionId::from_u128(99), &workspace);
    assert!(error.is_err(), "missing journal must fail resume");
}

#[test]
fn product_resume_recovers_an_open_operation() {
    let (home, workspace) = workspace_home();
    let mut driver = product::open(&open_spec(home.path(), &workspace)).expect("open must succeed");
    let session = journal::parse_path(driver.path()).expect("journal path must parse");
    driver
        .start_operation("killed mid-turn")
        .expect("operation must open");
    drop(driver);

    let mut resumed =
        product::resume(home.path(), session, &workspace).expect("resume must recover");
    assert!(
        resumed.state().operation.is_none(),
        "product resume must close a process-loss operation"
    );
    resumed
        .start_operation("continue")
        .expect("recovered session must accept a new turn");
    drop(resumed);
}

fn write_v1(path: &std::path::Path, user: &str, assistant: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("v1 parent must exist");
    }
    let mut log = crate::session::SessionLog::open(path).expect("v1 log must open");
    log.append_user(user).expect("v1 user must append");
    log.append_event(&crate::tools::Event::Assistant {
        text: assistant.to_string(),
    })
    .expect("v1 assistant must append");
}

#[test]
fn import_replays_v1_history_and_is_idempotent() {
    let (home, workspace) = workspace_home();
    let v1 = home.path().join("sessions/legacy-one.jsonl");
    write_v1(&v1, "hello from v1", "imported answer");
    let before = std::fs::read(&v1).expect("v1 bytes before import");

    let driver = product::import(home.path(), &v1, &workspace).expect("import must succeed");
    let replay = driver.replay();
    assert_eq!(replay.history.len(), 2);
    assert_eq!(replay.history[0].role, Role::User);
    assert_eq!(replay.history[0].content, "hello from v1");
    assert_eq!(replay.history[1].role, Role::Assistant);
    assert_eq!(replay.history[1].content, "imported answer");
    let first = driver.path().to_path_buf();
    drop(driver);

    let again = product::import(home.path(), &v1, &workspace).expect("second import must resume");
    assert_eq!(again.path(), first.as_path(), "second import must reuse v2");
    drop(again);

    let after = std::fs::read(&v1).expect("v1 bytes after import");
    assert_eq!(before, after, "import must not append to v1");
}

#[test]
fn import_converts_a_present_plan() {
    let (home, workspace) = workspace_home();
    let v1 = home.path().join("sessions/legacy-plan.jsonl");
    write_v1(&v1, "continue the plan", "ok");
    crate::plan::write(&workspace, "# Plan\n\n1. import\n").expect("plan must write");

    let driver =
        product::import(home.path(), &v1, &workspace).expect("import with plan must succeed");
    assert!(
        driver.state().plans.len() == 1,
        "imported plan must be one revision"
    );
    drop(driver);
}

#[test]
fn import_records_unmatched_legacy_tools() {
    let (home, workspace) = workspace_home();
    let v1 = home.path().join("sessions/legacy-open-tool.jsonl");
    if let Some(parent) = v1.parent() {
        std::fs::create_dir_all(parent).expect("v1 parent must exist");
    }
    let mut log = crate::session::SessionLog::open(&v1).expect("v1 log must open");
    log.append_user("read it").expect("v1 user must append");
    log.append_event(&crate::tools::Event::ToolCall {
        id: "orphan".to_string(),
        name: "read_file".to_string(),
        arguments: r#"{"path":"note.txt"}"#.to_string(),
    })
    .expect("open tool call must append");
    let driver = product::import(home.path(), &v1, &workspace).expect("import must succeed");
    let log = std::fs::read_to_string(driver.path()).expect("v2 journal must exist");
    assert!(
        log.contains("legacy_tool_uncertain"),
        "unmatched v1 tool must be uncertain: {log}"
    );
    drop(driver);
}

#[test]
fn import_blocks_a_malformed_plan() {
    let (home, workspace) = workspace_home();
    let v1 = home.path().join("sessions/legacy-bad-plan.jsonl");
    write_v1(&v1, "hello", "ok");
    std::fs::create_dir_all(workspace.join(".mage/plan.md")).expect("plan path is a directory");
    let error = product::import(home.path(), &v1, &workspace);
    assert!(error.is_err(), "malformed plan must block import");
}
