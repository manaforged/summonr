use std::process::Command;
use std::time::Duration;

use serde_json::{Value, json};

use crate::support::{Model, Session, Step};

fn events(stdout: &str) -> Vec<Value> {
    stdout
        .lines()
        .map(|line| serde_json::from_str(line).expect("the CLI must emit JSON events"))
        .collect()
}

pub(crate) fn a_plain_worktree_turn_lands_its_changes_without_running_a_check() {
    let session = Session::new();
    std::fs::create_dir(session.work.path().join(".summonr"))
        .expect("create the fixture check directory");
    std::fs::write(
        session.work.path().join(".summonr/check.toml"),
        "check = 'exit 1'\n",
    )
    .expect("write the fixture check");
    for args in [
        vec!["config", "user.name", "Test"],
        vec!["config", "user.email", "test@example.invalid"],
        vec!["add", "."],
        vec!["commit", "-qm", "fixture: record initial files"],
    ] {
        let status = Command::new("git")
            .args(args)
            .current_dir(session.work.path())
            .status()
            .expect("run fixture git");
        assert!(status.success());
    }
    let model = Model::start(vec![
        Step::Call(
            "write",
            json!({"path": "landed.txt", "content": "landed\n"}),
        ),
        Step::Text("finished"),
    ]);
    let result = session.run(&model, "", &["--worktree", "--json"]);
    assert_eq!(result.code, Some(0), "{}", result.stdout);
    assert_eq!(
        std::fs::read_to_string(session.work.path().join("landed.txt"))
            .expect("the completed worktree must land in the original checkout"),
        "landed\n"
    );
    let events = events(&result.stdout);
    assert!(
        events
            .iter()
            .any(|event| { event["type"] == "merged" && event["files"] == json!(["landed.txt"]) })
    );
    assert!(!events.iter().any(|event| event["type"] == "verify"));
    assert!(
        model
            .tool_results()
            .iter()
            .any(|text| text.contains("landed.txt"))
    );
    let (ok, text) = session.cli(&["worktrees", "--json"]);
    assert!(ok, "{text}");
    assert_eq!(
        serde_json::from_str::<Value>(&text).expect("worktree JSON"),
        json!([])
    );
}

pub(crate) fn forking_a_named_session_keeps_the_original_transcript() {
    let session = Session::new();
    let original = session.run(
        &Model::start(vec![Step::Text("original reply")]),
        "",
        &["--name", "source-session", "--json"],
    );
    assert_eq!(original.code, Some(0), "{}", original.stdout);
    let original_events = events(&original.stdout);
    let id = original_events[0]["id"].as_str().expect("session id");
    let journal = session
        .home
        .path()
        .join("sessions")
        .join(id)
        .join("session.jsonl");
    let before = std::fs::read(&journal).expect("read original transcript");
    let model = Model::start(vec![Step::Text("fork reply")]);
    let forked = session.run(
        &model,
        "",
        &["--session", "source-session", "--fork", "--json"],
    );
    assert_eq!(forked.code, Some(0), "{}", forked.stdout);
    let forked_events = events(&forked.stdout);
    assert_ne!(forked_events[0]["id"], original_events[0]["id"]);
    assert_eq!(
        std::fs::read(&journal).expect("reread original transcript"),
        before
    );
    assert!(model.requests()[0]["messages"]
        .as_array()
        .expect("sent conversation")
        .iter()
        .any(|message| message["role"] == "assistant" && message["content"] == "original reply"));
}

pub(crate) fn a_budget_abort_runs_the_failed_check_before_choosing_its_exit_code() {
    let session = Session::new();
    std::fs::create_dir(session.work.path().join(".summonr"))
        .expect("create the fixture check directory");
    std::fs::write(
        session.work.path().join(".summonr/check.toml"),
        "check = 'exit 1'\n",
    )
    .expect("write the failing check");
    let model = Model::start(vec![Step::After(
        Duration::from_secs(2),
        Box::new(Step::Text("too late")),
    )]);
    let result = session.run(
        &model,
        "",
        &["--goal", "finish the task", "--max-secs", "1", "--json"],
    );
    assert_eq!(result.code, Some(2), "{}", result.stdout);
    let events = events(&result.stdout);
    assert!(events.iter().any(|event| {
        event["type"] == "verify" && event["command"] == "exit 1" && event["passed"] == false
    }));
    assert!(
        events
            .iter()
            .any(|event| { event["type"] == "work" && event["progress"]["checks_failed"] == true })
    );
    assert!(
        model.requests()[0]["messages"]
            .as_array()
            .expect("sent conversation")
            .iter()
            .any(|message| message["role"] == "user"
                && message["content"]
                    .as_str()
                    .is_some_and(|text| text.contains("finish the task")))
    );
}
