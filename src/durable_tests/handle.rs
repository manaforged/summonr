//! Product-facing handle tests that drive the durable session through the
//! crate-level handle, exactly as the cutover's entry points will.

use crate::durable::driver::SessionDriver;
use crate::durable::effect::Usage;
use crate::durable::id::SessionId;
use crate::durable::journal::inspect;
use crate::durable::observe::workspace_identity_of;
use crate::durable::workspace::{PathKey, PathPlatform, WorkspaceScope};
use crate::provider::Role;

use super::fixture;

fn usage(input: u64, output: u64, millis: u64) -> Usage {
    Usage {
        input_tokens: Some(input),
        output_tokens: Some(output),
        cached_input_tokens: Some(0),
        live_millis: Some(millis),
    }
}

fn workspace_scope(workspace: &std::path::Path) -> WorkspaceScope {
    let identity = workspace_identity_of(workspace).expect("workspace identity must resolve");
    WorkspaceScope::new(
        fixture::id(10),
        identity,
        vec![PathKey::root(PathPlatform::Unix)],
        Vec::new(),
        Vec::new(),
    )
    .expect("scope must be valid")
}

#[test]
fn handle_runs_an_operation_round_and_replays() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace must be created");
    std::fs::write(workspace.join("src.txt"), b"hello").expect("workspace file must be written");
    let scope = workspace_scope(&workspace);

    let path = directory.path().join("session.jsonl");
    let mut handle = SessionDriver::open(&path, fixture::session(), &scope, fixture::head())
        .expect("handle must open");
    handle.set_workspace(&workspace);

    handle
        .start_operation("build the cutover")
        .expect("operation must open");
    let attempt = handle
        .start_provider(br#"{"model":"test-model"}"#, fixture::exposure())
        .expect("provider must start");
    handle
        .finish_provider(attempt, "done", "", usage(10, 5, 100))
        .expect("provider must finish");

    assert!(inspect(&path).is_ok(), "handle journal must replay");
    drop(handle);
}

#[test]
fn handle_resumes_and_opens_a_child() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace must be created");
    std::fs::write(workspace.join("src.txt"), b"hello").expect("workspace file must be written");
    let scope = workspace_scope(&workspace);

    let path = directory.path().join("session.jsonl");
    let mut handle = SessionDriver::open(&path, fixture::session(), &scope, fixture::head())
        .expect("handle must open");
    handle.set_workspace(&workspace);

    let child = SessionId::from_u128(2);
    handle.fork(child).expect("fork must open");
    drop(handle);

    let resumed = SessionDriver::resume(&path, fixture::session()).expect("handle must resume");
    assert!(resumed.state().open_fork.is_some(), "open fork must replay");
    drop(resumed);
}

#[test]
fn handle_projects_history_from_the_durable_journal() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut handle = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("handle must open");

    handle
        .start_operation("read the spec")
        .expect("operation must open");
    let attempt = handle
        .start_provider(br#"{"model":"test-model"}"#, fixture::exposure())
        .expect("provider must start");
    handle
        .finish_provider(attempt, "done", "", usage(10, 5, 100))
        .expect("provider must finish");

    let replay = handle.replay();
    assert_eq!(replay.history.len(), 2, "user and assistant must project");
    assert_eq!(replay.history[0].role, Role::User);
    assert_eq!(replay.history[0].content, "read the spec");
    assert_eq!(replay.history[1].role, Role::Assistant);
    assert_eq!(replay.history[1].content, "done");
    drop(handle);
}
