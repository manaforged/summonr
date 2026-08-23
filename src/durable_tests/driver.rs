use crate::durable::context::{AbortReason, InputOrigin};
use crate::durable::driver::{DriverError, SessionDriver};
use crate::durable::effect::{ReplayPolicy, Usage};
use crate::durable::fact::{ForkAdoption, LegacySource};
use crate::durable::id::SessionId;
use crate::durable::journal::inspect;
use crate::durable::observe::workspace_identity_of;
use crate::durable::workspace::{PathComponent, PathKey, PathPlatform, WorkspaceScope};
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

#[test]
fn start_operation_records_a_durable_group_that_replays() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let scope = fixture::scope();
    let head = fixture::head();

    let mut driver =
        SessionDriver::open(&path, fixture::session(), &scope, head).expect("driver must open");
    driver
        .start_operation("implement the durable driver")
        .expect("operation must open");

    assert!(inspect(&path).is_ok(), "journal must replay a valid prefix");
    assert_eq!(driver.state().seq(), 2);
    assert!(
        driver.state().hot_receipts() > 0,
        "start must store a command receipt"
    );
    drop(driver);

    // Replay from disk must restore the open operation, so a second start is
    // rejected as a duplicate open operation.
    let mut resumed = SessionDriver::resume(&path, fixture::session()).expect("resume must reopen");
    let result = resumed.start_operation("another");
    assert!(matches!(result, Err(DriverError::Busy)));
}

#[test]
fn start_provider_records_a_durable_round() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    driver
        .start_operation("read the spec")
        .expect("operation must open");

    let attempt = driver
        .start_provider(br#"{"model":"test-model"}"#)
        .expect("provider must start");

    assert!(inspect(&path).is_ok(), "journal must replay a valid prefix");
    drop((attempt, driver));
}

#[test]
fn finish_provider_closes_a_round() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    driver
        .start_operation("read the spec")
        .expect("operation must open");
    let attempt = driver
        .start_provider(br#"{"model":"test-model"}"#)
        .expect("provider must start");

    driver
        .finish_provider(attempt, "done", "", usage(10, 5, 100))
        .expect("provider must finish");

    assert!(inspect(&path).is_ok(), "journal must replay a valid prefix");
    drop(driver);
}

#[test]
fn set_work_records_a_plan_stage_work() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");

    driver
        .set_work("Implement durable execution")
        .expect("work must set");

    assert!(inspect(&path).is_ok(), "journal must replay a valid prefix");
    drop(driver);
}

#[test]
fn fork_records_an_intent_and_publishes_the_sealed_child() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace must be created");
    std::fs::write(workspace.join("src.txt"), b"hello").expect("workspace file must be written");
    let identity = workspace_identity_of(&workspace).expect("workspace identity must resolve");
    let scope = WorkspaceScope::new(
        fixture::id(10),
        identity,
        vec![PathKey::root(PathPlatform::Unix)],
        Vec::new(),
        Vec::new(),
    )
    .expect("scope must be valid");

    let path = directory.path().join("parent.jsonl");
    let child_path = directory.path().join("child.jsonl");
    let mut driver = SessionDriver::open(&path, fixture::session(), &scope, fixture::head())
        .expect("driver must open");
    driver.set_workspace(&workspace);

    let child = SessionId::from_u128(2);
    let intent = driver
        .fork(child, ForkAdoption::AdoptChild)
        .expect("fork must open");
    assert!(inspect(&path).is_ok(), "fork intent must replay");
    assert!(driver.state().open_fork.is_some());

    driver
        .publish_fork(intent, &child_path)
        .expect("fork must publish");
    assert!(inspect(&path).is_ok(), "published fork must replay");
    assert!(driver.state().open_fork.is_none());
    assert!(driver.state().forks.contains_key(&intent));
    let sealed = match driver.state().forks.get(&intent) {
        Some(crate::durable::fact::ForkOutcome::Published { sealed_journal, .. }) => {
            *sealed_journal
        }
        other => panic!("fork must publish a sealed child, got {other:?}"),
    };
    drop(driver);

    assert!(
        child_path.is_file(),
        "publish_fork must create the child journal"
    );
    let child_state = inspect(&child_path).expect("child journal must replay alone");
    assert_eq!(
        child_state.head(),
        Some(sealed),
        "parent must record the child chain head"
    );
    let child_driver =
        SessionDriver::resume(&child_path, child).expect("child must resume without the parent");
    drop(child_driver.replay());
    drop(child_driver);
}

fn legacy_source(length: u64) -> LegacySource {
    LegacySource {
        display_id: fixture::text("legacy-display-id"),
        format: 1,
        converter: 1,
        log_length: length,
        log_digest: fixture::digest("legacy"),
        plan_length: None,
        plan_digest: None,
    }
}

#[test]
fn open_legacy_anchors_the_source_and_accepts_a_divergence() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace must be created");
    std::fs::write(workspace.join("src.txt"), b"hello").expect("workspace file must be written");
    let identity = workspace_identity_of(&workspace).expect("workspace identity must resolve");
    let scope = WorkspaceScope::new(
        fixture::id(10),
        identity,
        vec![PathKey::root(PathPlatform::Unix)],
        Vec::new(),
        Vec::new(),
    )
    .expect("scope must be valid");

    let path = directory.path().join("legacy.jsonl");
    let mut driver = SessionDriver::open_legacy(
        &path,
        fixture::session(),
        &scope,
        fixture::head(),
        legacy_source(9),
        &workspace,
    )
    .expect("legacy root must open");

    driver
        .accept_legacy(legacy_source(12))
        .expect("legacy source must be accepted");

    assert!(inspect(&path).is_ok(), "legacy history must replay");
    drop(driver);
}

#[test]
fn select_plan_requires_a_workspace() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    driver
        .set_work("Implement durable execution")
        .expect("work must set");
    let seq = driver.state().seq();

    assert!(matches!(
        driver.select_plan("# Plan\n"),
        Err(DriverError::NoWorkspace)
    ));
    assert_eq!(driver.state().seq(), seq, "intent must not commit");
    drop(driver);
}

#[test]
fn select_plan_writes_the_plan_file() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let workspace = directory.path().join("workspace");
    std::fs::create_dir(&workspace).expect("workspace must be created");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    driver.set_workspace(&workspace);
    let work = driver
        .set_work("Implement durable execution")
        .expect("work must set");
    let markdown = "# Plan\n\nImplement the plan.";

    let revision = driver.select_plan(markdown).expect("plan must select");

    assert!(inspect(&path).is_ok(), "journal must replay a valid prefix");
    assert!(
        driver.state().open_materialization.is_none(),
        "materialization must settle"
    );
    assert!(
        driver
            .state()
            .materializations
            .values()
            .any(|materialization| materialization
                .outcome
                .as_ref()
                .is_some_and(crate::durable::fact::MaterializationOutcome::usable)),
        "Plan file must record a usable outcome"
    );
    let destination =
        crate::durable::work::plan_path(PathPlatform::Unix, fixture::session(), work, revision)
            .expect("Plan path must be valid");
    let mut file = workspace;
    for component in destination.components() {
        match component {
            PathComponent::Unix(bytes) => file.push(std::ffi::OsStr::new(
                std::str::from_utf8(bytes.as_slice()).expect("path must be UTF-8 in this test"),
            )),
            PathComponent::Windows(_) => panic!("unix fixture must not use windows paths"),
        }
    }
    assert_eq!(
        std::fs::read(&file).expect("Plan file must exist"),
        markdown.as_bytes()
    );
    drop(driver);
}

#[test]
fn tool_round_plans_starts_and_finishes_a_tool() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    driver
        .start_operation("read the spec")
        .expect("operation must open");
    let attempt = driver
        .start_provider(br#"{"model":"test-model"}"#)
        .expect("provider must start");

    let call = driver
        .finish_provider_with_tool(
            attempt,
            "done",
            "",
            usage(10, 5, 100),
            "read_file",
            r#"{"path":"README.md"}"#,
        )
        .expect("provider must finish and plan a tool");
    driver
        .start_tool(call, r#"{"path":"README.md"}"#, "read_file")
        .expect("tool must start");
    driver
        .finish_tool(call, "# Mage", usage(4, 2, 20))
        .expect("tool must finish");
    driver.close_operation().expect("operation must close");

    assert!(inspect(&path).is_ok(), "journal must replay a valid prefix");
    drop(driver);
}

#[test]
fn tool_round_replays_issued_name_and_arguments() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    driver
        .start_operation("read the spec")
        .expect("operation must open");
    let attempt = driver
        .start_provider(br#"{"model":"test-model"}"#)
        .expect("provider must start");
    let arguments = r#"{"path":"Agents.md"}"#;
    let call = driver
        .finish_provider_with_tool(
            attempt,
            "calling read_file",
            "need the agents guide",
            usage(12, 6, 80),
            "read_file",
            arguments,
        )
        .expect("provider must finish and plan a tool");
    driver
        .start_tool(call, arguments, "read_file")
        .expect("tool must start");
    driver
        .finish_tool(call, "# Agents", usage(3, 1, 15))
        .expect("tool must finish");

    let replay = driver.replay();
    assert_eq!(
        replay.history.len(),
        3,
        "user, assistant, and tool must project"
    );
    assert_eq!(replay.history[1].role, Role::Assistant);
    assert_eq!(replay.history[1].tool_calls.len(), 1);
    assert_eq!(replay.history[1].tool_calls[0].function.name, "read_file");
    assert_eq!(
        replay.history[1].tool_calls[0].function.arguments,
        arguments
    );
    assert_eq!(
        replay.history[1].reasoning.as_deref(),
        Some("need the agents guide")
    );
    assert_eq!(replay.history[2].role, Role::Tool);
    assert_eq!(replay.history[2].content, "# Agents");
    assert!(inspect(&path).is_ok(), "tool payload journal must replay");
    drop(driver);
}

#[test]
fn tool_batch_plans_and_finishes_every_call() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    driver
        .start_operation("read both files")
        .expect("operation must open");
    let attempt = driver
        .start_provider(br#"{"model":"test-model"}"#)
        .expect("provider must start");
    let calls = driver
        .finish_provider_with_tools(
            attempt,
            "calling two reads",
            "",
            usage(10, 5, 100),
            &[
                ("read_file", r#"{"path":"a.md"}"#, "c1"),
                ("read_file", r#"{"path":"b.md"}"#, "c2"),
            ],
        )
        .expect("provider must plan both tools");
    assert_eq!(calls.len(), 2, "one planned id per provider call");
    driver
        .start_tool(calls[0], r#"{"path":"a.md"}"#, "read_file")
        .expect("first tool must start");
    driver
        .start_tool(calls[1], r#"{"path":"b.md"}"#, "read_file")
        .expect("second tool must start");
    driver
        .finish_tools(&[
            (calls[0], "A", usage(1, 1, 10)),
            (calls[1], "B", usage(1, 1, 10)),
        ])
        .expect("batch must finish together");

    let replay = driver.replay();
    assert_eq!(replay.history.len(), 4, "user, assistant, two tools");
    assert_eq!(replay.history[1].tool_calls.len(), 2);
    assert_eq!(replay.history[1].tool_calls[0].id, "c1");
    assert_eq!(replay.history[1].tool_calls[1].id, "c2");
    assert_eq!(replay.history[2].content, "A");
    assert_eq!(replay.history[3].content, "B");
    assert!(inspect(&path).is_ok(), "two-tool batch must replay");
    drop(driver);
}

#[test]
fn close_operation_closes_the_round() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    driver
        .start_operation("read the spec")
        .expect("operation must open");
    let attempt = driver
        .start_provider(br#"{"model":"test-model"}"#)
        .expect("provider must start");
    driver
        .finish_provider(attempt, "done", "", usage(10, 5, 100))
        .expect("provider must finish");

    driver.close_operation().expect("operation must close");

    assert!(inspect(&path).is_ok(), "journal must replay a valid prefix");
    drop(driver);
}

#[test]
fn provider_requires_an_open_operation() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");

    assert!(matches!(
        driver.start_provider(br#"{"model":"test-model"}"#),
        Err(DriverError::NoOperation)
    ));
}

#[test]
fn resume_continues_an_open_operation() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    driver
        .start_operation("implement the durable driver")
        .expect("operation must open");
    drop(driver);

    let mut resumed = SessionDriver::resume(&path, fixture::session()).expect("resume must reopen");
    let attempt = resumed
        .start_provider(br#"{"model":"test-model"}"#)
        .expect("resume must continue the open operation");
    resumed
        .finish_provider(attempt, "done", "", usage(10, 5, 100))
        .expect("resumed provider must finish");
    assert!(inspect(&path).is_ok(), "resumed round must replay");
    let replay = resumed.replay();
    assert_eq!(replay.history.len(), 2, "user and assistant must project");
    assert_eq!(replay.history[0].content, "implement the durable driver");
    assert_eq!(replay.history[1].content, "done");
    drop(resumed);
}

#[test]
fn resume_finishes_an_open_provider_round() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    driver
        .start_operation("read the spec")
        .expect("operation must open");
    let attempt = driver
        .start_provider(br#"{"model":"test-model"}"#)
        .expect("provider must start");
    drop(driver);

    let mut resumed = SessionDriver::resume(&path, fixture::session()).expect("resume must reopen");
    resumed
        .finish_provider(attempt, "done", "", usage(10, 5, 100))
        .expect("resume must finish the open provider attempt");
    resumed.close_operation().expect("operation must close");
    assert!(inspect(&path).is_ok(), "resumed finish must replay");
    drop(resumed);
}

#[test]
fn abort_cancels_an_open_provider_round() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    driver
        .start_operation("read the spec")
        .expect("operation must open");
    let _attempt = driver
        .start_provider(br#"{"model":"test-model"}"#)
        .expect("provider must start");
    driver
        .abort(AbortReason::User)
        .expect("abort must settle and close");
    assert!(
        driver.state().operation.is_none(),
        "abort must close the operation"
    );
    let log = std::fs::read_to_string(&path).expect("journal must exist");
    assert!(
        log.contains("abort_requested"),
        "abort must journal AbortRequested: {log}"
    );
    assert!(inspect(&path).is_ok(), "aborted journal must replay");
    drop(driver);
}

#[test]
fn write_file_starts_as_never_replay() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    driver
        .start_operation("write the file")
        .expect("operation must open");
    let attempt = driver
        .start_provider(br#"{"model":"test-model"}"#)
        .expect("provider must start");
    let call = driver
        .finish_provider_with_tool(
            attempt,
            "writing",
            "",
            usage(4, 2, 10),
            "write_file",
            r#"{"path":"a.rs","content":"x"}"#,
        )
        .expect("write_file must be planned");
    driver
        .start_tool(call, r#"{"path":"a.rs","content":"x"}"#, "write_file")
        .expect("write_file must start");
    let replay = driver
        .state()
        .tools
        .values()
        .next()
        .and_then(|tool| tool.intent.as_ref())
        .map(|intent| intent.replay);
    assert_eq!(replay, Some(ReplayPolicy::Never));
    let footprint = driver
        .state()
        .tools
        .values()
        .next()
        .and_then(|tool| tool.intent.as_ref())
        .map(|intent| &intent.footprint);
    assert!(
        matches!(
            footprint,
            Some(crate::durable::effect::WorkspaceFootprint::Exact(_))
        ),
        "write_file must record its path before spawn"
    );
    drop(driver);
}

#[test]
fn run_command_starts_with_a_reserved_process() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    driver
        .start_operation("run the check")
        .expect("operation must open");
    let attempt = driver
        .start_provider(br#"{"model":"test-model"}"#)
        .expect("provider must start");
    let call = driver
        .finish_provider_with_tool(
            attempt,
            "running",
            "",
            usage(4, 2, 10),
            "run_command",
            r#"{"command":"true"}"#,
        )
        .expect("run_command must be planned");
    driver
        .start_tool(call, r#"{"command":"true"}"#, "run_command")
        .expect("run_command must start");
    let process = driver
        .state()
        .tools
        .values()
        .next()
        .and_then(|tool| tool.intent.as_ref())
        .and_then(|intent| intent.process.as_ref());
    assert!(
        matches!(
            process,
            Some(crate::durable::effect::ProcessRecord::Reserved { .. })
        ),
        "run_command must reserve process identity before spawn"
    );
    drop(driver);
}

#[test]
fn background_command_opens_a_detached_job() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    driver
        .start_operation("run in the background")
        .expect("operation must open");
    let attempt = driver
        .start_provider(br#"{"model":"test-model"}"#)
        .expect("provider must start");
    let arguments = r#"{"command":"sleep 1","background":true}"#;
    let call = driver
        .finish_provider_with_tool(
            attempt,
            "running",
            "",
            usage(4, 2, 10),
            "run_command",
            arguments,
        )
        .expect("background command must be planned");
    driver
        .start_tool(call, arguments, "run_command")
        .expect("background command must start");
    driver
        .finish_tool(
            call,
            "started job abc\npid 4242\nlog /tmp/job.log\nstate: running",
            usage(1, 0, 5),
        )
        .expect("background command must finish");
    assert!(
        !driver.state().jobs.is_empty(),
        "background run_command must open a detached job"
    );
    assert!(inspect(&path).is_ok(), "journal must replay");
    drop(driver);
}

#[test]
fn resume_after_many_operations_allocates_fresh_ids() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    for index in 0..25 {
        driver
            .start_operation(&format!("round {index}"))
            .expect("operation must open");
        let attempt = driver
            .start_provider(br#"{"model":"test-model"}"#)
            .expect("provider must start");
        driver
            .finish_provider(attempt, "done", "", usage(10, 5, 100))
            .expect("provider must finish");
        driver.close_operation().expect("operation must close");
    }
    drop(driver);

    let mut resumed = SessionDriver::resume(&path, fixture::session()).expect("resume must reopen");
    resumed
        .start_operation("after many rounds")
        .expect("fresh operation must open after resume");
    let attempt = resumed
        .start_provider(br#"{"model":"test-model"}"#)
        .expect("fresh provider must start");
    resumed
        .finish_provider(attempt, "done", "", usage(10, 5, 100))
        .expect("fresh provider must finish");
    resumed
        .close_operation()
        .expect("fresh operation must close");
    assert!(inspect(&path).is_ok(), "post-resume history must replay");
    drop(resumed);
}

#[test]
fn empty_input_is_rejected_before_any_commit() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");

    assert!(matches!(
        driver.start_operation("   "),
        Err(DriverError::Empty)
    ));
    assert_eq!(driver.state().seq(), 1);
}

#[test]
fn steer_survives_resume_as_the_through_entry() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    driver
        .start_operation("implement the durable driver")
        .expect("operation must open");
    driver
        .steer("prefer the short path", InputOrigin::User)
        .expect("steer must apply");
    let seq = driver.state().seq();
    assert!(matches!(
        driver.steer("   ", InputOrigin::User),
        Err(DriverError::Empty)
    ));
    assert_eq!(driver.state().seq(), seq, "empty steer must not commit");
    drop(driver);

    let mut resumed = SessionDriver::resume(&path, fixture::session()).expect("resume must reopen");
    let attempt = resumed
        .start_provider(br#"{"model":"test-model"}"#)
        .expect("provider must start through the steer entry");
    resumed
        .finish_provider(attempt, "done", "", usage(10, 5, 100))
        .expect("provider must finish");
    let replay = resumed.replay();
    assert_eq!(
        replay.history.len(),
        3,
        "start, steer, and assistant must project"
    );
    assert_eq!(replay.history[0].content, "implement the durable driver");
    assert_eq!(replay.history[1].content, "prefer the short path");
    assert_eq!(replay.history[2].content, "done");
    assert!(inspect(&path).is_ok(), "steered journal must replay");
    drop(resumed);
}

#[test]
fn compact_during_an_open_operation_rebases_for_the_next_provider() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    driver
        .start_operation("implement the durable driver")
        .expect("operation must open");
    let attempt = driver
        .start_provider(br#"{"model":"test-model"}"#)
        .expect("provider must start");
    driver
        .finish_provider(attempt, "long answer", "", usage(10, 5, 100))
        .expect("provider must finish");
    let before = driver.state().current_epoch.expect("epoch must exist");

    driver
        .compact("Keep the short path.")
        .expect("compact must rebase at a round boundary");
    let after = driver
        .state()
        .current_epoch
        .expect("replacement epoch must exist");
    assert_ne!(before, after, "compact must open a new epoch");
    drop(driver);

    let mut resumed = SessionDriver::resume(&path, fixture::session()).expect("resume must reopen");
    let attempt = resumed
        .start_provider(br#"{"model":"test-model"}"#)
        .expect("provider must start on the compacted epoch");
    resumed
        .finish_provider(attempt, "done", "", usage(8, 3, 40))
        .expect("provider must finish after compact");
    let replay = resumed.replay();
    assert_eq!(replay.history[0].content, "implement the durable driver");
    assert_eq!(replay.history[1].role, Role::System);
    assert_eq!(replay.history[1].content, "Keep the short path.");
    assert_eq!(replay.history[2].content, "done");
    assert!(inspect(&path).is_ok(), "compacted journal must replay");
    drop(resumed);
}

#[test]
fn clear_replaces_an_idle_conversation_and_rejects_an_open_operation() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    driver
        .start_operation("read the spec")
        .expect("operation must open");
    assert!(matches!(driver.clear(), Err(DriverError::Busy)));
    let attempt = driver
        .start_provider(br#"{"model":"test-model"}"#)
        .expect("provider must start");
    driver
        .finish_provider(attempt, "done", "", usage(10, 5, 100))
        .expect("provider must finish");
    driver.close_operation().expect("operation must close");
    let before = driver.state().current_epoch.expect("epoch must exist");

    driver.clear().expect("idle clear must replace the epoch");
    let after = driver
        .state()
        .current_epoch
        .expect("replacement epoch must exist");
    assert_ne!(before, after, "clear must open a replacement epoch");
    assert!(
        driver.replay().history.is_empty(),
        "clear must drop history"
    );
    assert!(inspect(&path).is_ok(), "cleared journal must replay");
    drop(driver);
}

#[test]
fn peer_steer_applies_while_an_operation_is_open() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    assert!(matches!(
        driver.steer("no operation", InputOrigin::Peer),
        Err(DriverError::NoOperation)
    ));
    driver
        .start_operation("read the spec")
        .expect("operation must open");
    driver
        .steer("from a peer", InputOrigin::Peer)
        .expect("peer steer must apply");
    assert!(matches!(
        driver.start_operation("another"),
        Err(DriverError::Busy)
    ));
    let replay = driver.replay();
    assert_eq!(replay.history[1].content, "from a peer");
    assert!(inspect(&path).is_ok(), "peer steer journal must replay");
    drop(driver);
}

#[test]
fn progress_snapshot_survives_resume() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    driver.set_work("ship the cutover").expect("work must open");
    driver
        .progress(r#"{"next_step":2,"goal":"ship"}"#)
        .expect("progress must journal");
    drop(driver);

    let resumed = SessionDriver::resume(&path, fixture::session()).expect("resume must reopen");
    assert_eq!(
        resumed.progress_text(),
        Some(r#"{"next_step":2,"goal":"ship"}"#)
    );
    drop(resumed);
}

#[test]
fn recover_closes_an_open_operation() {
    let directory = tempfile::tempdir().expect("temporary directory must exist");
    let path = directory.path().join("session.jsonl");
    let mut driver = SessionDriver::open(
        &path,
        fixture::session(),
        &fixture::scope(),
        fixture::head(),
    )
    .expect("driver must open");
    driver
        .start_operation("interrupted work")
        .expect("operation must open");
    drop(driver);

    let mut resumed = SessionDriver::resume(&path, fixture::session()).expect("resume must reopen");
    resumed
        .recover()
        .expect("recover must abort the open operation");
    assert!(
        resumed.state().operation.is_none(),
        "recover must leave the session idle"
    );
    resumed
        .start_operation("next turn")
        .expect("idle session must accept a new operation");
    drop(resumed);
}
