use super::*;
use crate::provider::{ToolFunction, ToolKind};

#[test]
fn a_compacted_tail_replays_as_the_model_saw_it_after_an_interrupted_reply() {
    let dir = tempfile::tempdir().unwrap();
    let mut driver = SessionDriver::open(dir.path(), dir.path(), "model", "system", None).unwrap();
    let exposure = || Exposure {
        max_input_tokens: 0,
        max_output_tokens: 0,
    };
    let call = ToolCall {
        id: "c1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "read".to_string(),
            arguments: "{}".to_string(),
        },
        continuation: None,
    };
    driver.prepare_turn_work("task", &[]).unwrap();
    driver.start_input(&UserInput::from("task")).unwrap();
    let attempt = driver
        .start_round(Purpose::Agent, "model", exposure())
        .unwrap();
    let planned = driver
        .finish_provider_with_tools(
            attempt,
            "",
            "",
            Usage::default(),
            &[call],
            UsageMeta::default(),
        )
        .unwrap();
    driver
        .finish_tool(planned[0], "file body", true, false, 1, None)
        .unwrap();
    let seen = driver.history()[1..].to_vec();
    let attempt = driver
        .start_round(Purpose::Agent, "model", exposure())
        .unwrap();
    driver.partial.text = "partial reply".to_string();
    driver
        .fail_provider(attempt, "stream cut", 0, false)
        .unwrap();
    driver.compact("summary", &seen).unwrap();
    assert_eq!(&driver.history()[2..], seen.as_slice());
}

#[test]
fn todo_list_does_not_outlive_clear() {
    let dir = tempfile::tempdir().unwrap();
    let mut driver = SessionDriver::open(dir.path(), dir.path(), "model", "system", None).unwrap();
    driver.prepare_turn_work("task", &[]).unwrap();
    driver
        .apply_todos(vec![crate::tools::Todo {
            text: "old task".to_string(),
            status: crate::tools::TodoStatus::Active,
        }])
        .unwrap();
    driver.clear().unwrap();
    driver.prepare_turn_work("next", &[]).unwrap();
    assert!(driver.work.as_ref().unwrap().todos.is_empty());
}

#[test]
fn steering_a_typed_steer_exports_whole_as_the_users_message() {
    let dir = tempfile::tempdir().unwrap();
    let mut driver = SessionDriver::open(dir.path(), dir.path(), "model", "system", None).unwrap();
    driver.prepare_turn_work("task", &[]).unwrap();
    driver.start_input(&UserInput::from("task")).unwrap();
    let typed = "Use staging.\nDo not run the migration.";
    driver
        .steer(&UserInput {
            text: typed.to_string(),
            images: Vec::new(),
            source: Some(crate::provider::Source {
                text: typed.to_string(),
                images: 0,
            }),
        })
        .unwrap();
    let exported = crate::session::export(driver.path()).unwrap();
    assert!(
        exported.contains(&format!("## You\n\n{typed}")),
        "{exported}"
    );
}

#[test]
fn a_cut_reply_records_the_usage_the_provider_reported() {
    let dir = tempfile::tempdir().unwrap();
    let mut driver = SessionDriver::open(dir.path(), dir.path(), "model", "system", None).unwrap();
    driver.prepare_turn_work("task", &[]).unwrap();
    driver.start_input(&UserInput::from("task")).unwrap();
    let exposure = Exposure {
        max_input_tokens: 9,
        max_output_tokens: 9,
    };
    let attempt = driver
        .start_round(Purpose::Agent, "model", exposure)
        .unwrap();
    driver.partial.usage = Some(crate::provider::Usage {
        prompt_tokens: 150_000,
        ..Default::default()
    });
    driver
        .fail_provider(attempt, "stream cut", 5, true)
        .unwrap();
    let text = std::fs::read_to_string(driver.log.path()).unwrap();
    let charge = text
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .rfind(|line| line["kind"] == "charge")
        .unwrap();
    assert_eq!(charge["reported"], true, "{charge}");
    assert_eq!(charge["usage"]["input"], 150_000, "{charge}");
}

#[test]
fn an_abort_closes_the_open_turn_with_its_reason() {
    let dir = tempfile::tempdir().unwrap();
    let mut driver = SessionDriver::open(dir.path(), dir.path(), "model", "system", None).unwrap();
    driver.prepare_turn_work("task", &[]).unwrap();
    driver.start_input(&UserInput::from("task")).unwrap();
    assert!(driver.turn_open());
    let abort = crate::tools::Abort::time_limit();
    driver.abort(&abort, None).unwrap();
    assert!(!driver.turn_open());
    assert_eq!(driver.last_close_reason(), Some(abort.reason.as_str()));
}

#[test]
fn the_next_user_line_consumes_a_cancel_reason() {
    let dir = tempfile::tempdir().unwrap();
    let mut driver = SessionDriver::open(dir.path(), dir.path(), "model", "system", None).unwrap();
    driver.prepare_turn_work("task", &[]).unwrap();
    driver.start_input(&UserInput::from("task")).unwrap();
    let abort = crate::tools::Abort::time_limit();
    driver.abort(&abort, None).unwrap();
    assert_eq!(driver.last_close_reason(), Some(abort.reason.as_str()));
    driver.start_input(&UserInput::from("next")).unwrap();
    assert_eq!(driver.last_close_reason(), None);
}

#[test]
fn a_policy_denied_check_stops() {
    let dir = tempfile::tempdir().unwrap();
    let mut driver = SessionDriver::open(dir.path(), dir.path(), "model", "system", None).unwrap();
    let mut work = Work::new("make the check pass", WorkKind::Goal);
    work.status = crate::durable::work::Status::Active(Stage::Verify);
    driver.work = Some(work);
    driver.turn = Some(Turn {
        stage: Some(Stage::Verify),
        round: None,
        calls: Vec::new(),
    });
    let receipt = crate::verify::Receipt {
        changes: Vec::new(),
        checks: vec![crate::verify::Outcome {
            command: "cargo test".to_string(),
            passed: false,
            output: "policy denied: command blocked".to_string(),
        }],
        snapshot: String::new(),
        aborted: false,
        verified: false,
        text: String::new(),
    };
    driver
        .commit_verdict(
            Verdict {
                passed: false,
                diagnostic: None,
            },
            Decision::Automatic,
            Some(&receipt),
        )
        .unwrap();
    let work = driver.work.as_ref().unwrap();
    assert!(
        matches!(
            work.status,
            crate::durable::work::Status::Abandoned(ref reason) if reason.contains("denied")
        ),
        "{:?}",
        work.status
    );
    assert!(
        !work.pending.iter().any(|line| line.contains("Repair")),
        "{:?}",
        work.pending
    );
}

#[test]
fn the_snapshot_skips_the_session_store_and_the_tree_cache() {
    let dir = tempfile::tempdir().unwrap();
    let driver = SessionDriver::open(dir.path(), dir.path(), "model", "system", None).unwrap();
    let cache = tree::cache_dir(&canonical(dir.path()));
    assert_eq!(driver.tree_cache(), Some(cache.clone()));
    let excludes = driver.excludes();
    assert!(excludes.contains(&cache), "{excludes:?}");
    assert!(
        excludes.contains(&canonical(
            driver.path().parent().unwrap().parent().unwrap()
        )),
        "{excludes:?}"
    );
}

#[test]
fn a_checkpoint_writes_the_tree_sidecar_when_the_trees_changed() {
    let dir = tempfile::tempdir().unwrap();
    let mut driver = SessionDriver::open(dir.path(), dir.path(), "model", "system", None).unwrap();
    let mut work = Work::new("ship", WorkKind::Single);
    work.base = Some(tree::snapshot(dir.path(), &[], None).unwrap());
    driver.work = Some(work);
    driver.checkpoint().unwrap();
    let sidecar = log::work_path(driver.path()).with_extension("tree.json");
    assert!(sidecar.is_file());
    std::fs::remove_file(&sidecar).unwrap();
    driver.checkpoint().unwrap();
    assert!(!sidecar.exists(), "unchanged trees are not rewritten");
}

#[test]
fn a_new_sentence_replaces_single_work() {
    let dir = tempfile::tempdir().unwrap();
    let mut driver = SessionDriver::open(dir.path(), dir.path(), "model", "system", None).unwrap();
    driver.prepare_turn_work("fix the meter", &[]).unwrap();
    assert_eq!(driver.work.as_ref().unwrap().objective, "fix the meter");
    driver.prepare_turn_work("why did that stall", &[]).unwrap();
    assert_eq!(
        driver.work.as_ref().unwrap().objective,
        "why did that stall"
    );
}

#[test]
fn a_goal_keeps_its_objective_when_the_next_sentence_differs() {
    let dir = tempfile::tempdir().unwrap();
    let mut driver = SessionDriver::open(dir.path(), dir.path(), "model", "system", None).unwrap();
    driver
        .prepare_external_work("keep this goal", WorkKind::Goal, &[])
        .unwrap();
    driver.prepare_turn_work("a side question", &[]).unwrap();
    assert_eq!(driver.work.as_ref().unwrap().objective, "keep this goal");
}

#[test]
fn a_second_goal_names_the_one_still_active_and_how_to_end_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut driver = SessionDriver::open(dir.path(), dir.path(), "model", "system", None).unwrap();
    driver
        .prepare_external_work("add a --version flag", WorkKind::Goal, &[])
        .unwrap();
    let error = driver
        .prepare_external_work("print hello", WorkKind::Goal, &[])
        .unwrap_err()
        .to_string();
    assert!(error.contains("add a --version flag"), "{error}");
    assert!(error.contains("/goal clear"), "{error}");
    driver
        .prepare_external_work("add a --version flag", WorkKind::Goal, &[])
        .unwrap();
}

#[test]
fn a_resumed_session_keeps_a_finished_tool_result_and_closes_the_cut_turn() {
    let dir = tempfile::tempdir().unwrap();
    let call = |id: &str| ToolCall {
        id: id.to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "read".to_string(),
            arguments: "{}".to_string(),
        },
        continuation: None,
    };
    let id = {
        let mut driver =
            SessionDriver::open(dir.path(), dir.path(), "model", "system", None).unwrap();
        driver.prepare_turn_work("task", &[]).unwrap();
        driver.start_input(&UserInput::from("task")).unwrap();
        let attempt = driver
            .start_round(
                Purpose::Agent,
                "model",
                Exposure {
                    max_input_tokens: 0,
                    max_output_tokens: 0,
                },
            )
            .unwrap();
        let planned = driver
            .finish_provider_with_tools(
                attempt,
                "",
                "",
                Usage::default(),
                &[call("done"), call("cut")],
                UsageMeta::default(),
            )
            .unwrap();
        driver
            .finish_tool(planned[0], "file body", true, false, 1, None)
            .unwrap();
        driver.id()
    };
    drop(crate::durable::store::lock(&log::session_path(dir.path(), id), true).unwrap());
    let resumed = SessionDriver::resume(dir.path(), id, dir.path()).unwrap();
    assert!(!resumed.turn_open());
    assert_eq!(
        resumed.last_close_reason(),
        Some("process stopped before the turn finished")
    );
    let results: Vec<&str> = resumed
        .history()
        .iter()
        .filter(|message| message.tool_call_id.as_deref() == Some("done"))
        .map(|message| message.content.as_str())
        .collect();
    assert_eq!(results, ["file body"]);
}
