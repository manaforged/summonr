//! Product constructor tests for format-2 open and resume.

use crate::durable::effect::Usage;
use crate::durable::id::SessionId;
use crate::durable::journal;
use crate::durable::product::{self, Open};
use crate::durable::work::{WorkStage, WorkStatus};
use crate::provider::Role;

use super::fixture;

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
        .start_provider(br#"{"model":"test-model"}"#, fixture::exposure())
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
        .start_provider(br#"{"model":"test-model"}"#, fixture::exposure())
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

#[test]
fn product_resume_returns_interrupted_verify_to_execute() {
    assert_verify_recovery(true);
}

#[test]
fn product_resume_recovers_between_execute_close_and_verify_open() {
    assert_verify_recovery(false);
}

#[test]
fn root_gitignore_negation_preserves_unrelated_directory_inference() {
    let (home, workspace) = workspace_home();
    std::fs::write(
        workspace.join(".gitignore"),
        "/generated/\n!generated/keep/\n/target\n",
    )
    .expect("gitignore must be written");

    let driver = product::open(&open_spec(home.path(), &workspace)).expect("open must succeed");
    let scope = driver
        .state()
        .default_scope
        .and_then(|id| driver.state().scopes.get(&id))
        .expect("default scope must exist");

    let names = scope
        .reserved()
        .iter()
        .filter_map(|path| path.path().last())
        .filter_map(|component| match component {
            crate::durable::workspace::PathComponent::Unix(value) => {
                std::str::from_utf8(value.as_slice()).ok()
            }
            crate::durable::workspace::PathComponent::Windows(_) => None,
        })
        .collect::<Vec<_>>();
    assert!(names.contains(&"target"), "{names:?}");
    assert!(!names.contains(&"generated"), "{names:?}");
}

fn assert_verify_recovery(open_verify: bool) {
    let (home, workspace) = workspace_home();
    let mut driver = product::open(&open_spec(home.path(), &workspace)).expect("open must succeed");
    let path = driver.path().to_path_buf();
    let session = journal::parse_path(&path).expect("journal path must parse");
    driver
        .prepare_turn_work(Some("change the source"), &[])
        .expect("Work must prepare");
    driver
        .start_input("change the source", &[])
        .expect("Execute must open");
    let attempt = driver
        .start_provider(br#"{"model":"test-model"}"#, fixture::exposure())
        .expect("provider must start");
    driver
        .finish_provider(attempt, "done", "", usage())
        .expect("provider must finish");
    std::fs::write(workspace.join("src.txt"), b"changed").expect("source must change");
    driver.enter_verify().expect("candidate must capture");
    if open_verify {
        driver
            .start_verification()
            .expect("Verify operation must open");
    }
    drop(driver);

    let resumed = product::resume(home.path(), session, &workspace).expect("resume must recover");
    let work = resumed
        .state()
        .works
        .values()
        .find(|work| !work.status().terminal())
        .expect("Work must remain active");
    assert_eq!(work.status(), &WorkStatus::Active(WorkStage::Execute));
    assert!(work.candidate().is_none());
    assert!(resumed.state().operation.is_none());
}
