use std::process::Command;
use std::time::Duration;

use serde_json::{Value, json};

use crate::support::{Model, Options, Session, Step, advertised};

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
        vec!["config", "core.autocrlf", "false"],
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

fn with_check(session: &Session, check: &str) {
    std::fs::create_dir_all(session.work.path().join(".summonr")).expect("check directory");
    std::fs::write(
        session.work.path().join(".summonr/check.toml"),
        format!("check = '{check}'\n"),
    )
    .expect("write the check");
}

fn commit_all(session: &Session) {
    for args in [
        vec!["config", "user.name", "Test"],
        vec!["config", "user.email", "test@example.invalid"],
        vec!["config", "core.autocrlf", "false"],
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
}

pub(crate) fn a_failed_worktree_run_without_changes_says_so() {
    let budget = crate::support::slow(2).to_string();
    let session = Session::new();
    with_check(&session, "exit 1");
    commit_all(&session);
    let model = Model::start(vec![]);
    let result = session.run(
        &model,
        "",
        &[
            "--worktree",
            "--goal",
            "finish",
            "--max-secs",
            &budget,
            "--json",
        ],
    );
    assert_eq!(result.code, Some(2), "{}", result.stdout);
    let stderr = crate::support::read(&session.file("stderr"));
    assert!(stderr.contains("the run made no changes there"), "{stderr}");
    assert!(!stderr.contains("uncommitted"), "{stderr}");
}

pub(crate) fn a_goal_run_reports_the_judge_usage_once() {
    let session = Session::new();
    with_check(&session, "exit 0");
    let model = Model::start(vec![
        Step::Text("fixed"),
        Step::Text(r#"{"verdict": "done", "reason": "the check passed"}"#),
    ]);
    let result = session.run(&model, "", &["--goal", "fix it", "--json"]);
    assert_eq!(result.code, Some(0), "{}", result.stdout);
    let usage = events(&result.stdout)
        .iter()
        .filter(|event| event["type"] == "usage")
        .count();
    assert_eq!(usage, model.requests().len(), "{}", result.stdout);
}

pub(crate) fn a_goal_recovers_after_more_than_three_failed_checks() {
    let session = Session::new();
    with_check(&session, "git diff --exit-code -- marker.txt");
    std::fs::write(session.work.path().join("marker.txt"), "clean\n").expect("write clean fixture");
    commit_all(&session);
    std::fs::write(session.work.path().join("marker.txt"), "dirty\n").expect("break the check");
    let model = Model::start(vec![
        Step::Text("first attempt"),
        Step::Call(
            "write",
            json!({"path": "marker.txt", "content": "dirty two\n"}),
        ),
        Step::Text("second attempt"),
        Step::Call(
            "write",
            json!({"path": "marker.txt", "content": "dirty three\n"}),
        ),
        Step::Text("third attempt"),
        Step::Call(
            "write",
            json!({"path": "marker.txt", "content": "dirty four\n"}),
        ),
        Step::Text("fourth attempt"),
        Step::Call("write", json!({"path": "marker.txt", "content": "clean\n"})),
        Step::Text("repaired"),
        Step::Text(r#"{"verdict":"done","reason":"the original check passes"}"#),
    ]);
    let result = session.run(&model, "", &["--goal", "repair marker.txt", "--json"]);
    assert_eq!(result.code, Some(0), "{}", result.stdout);
    let events = events(&result.stdout);
    assert_eq!(
        events
            .iter()
            .filter(|event| event["type"] == "verify" && event["passed"] == false)
            .count(),
        4
    );
    assert!(
        events
            .iter()
            .any(|event| event["type"] == "verify" && event["passed"] == true)
    );
    let requests = model.requests();
    let repair = &requests[requests.len() - 2]["messages"];
    assert!(repair.to_string().contains("marker.txt"), "{repair}");
    assert!(
        repair
            .as_array()
            .expect("repair conversation")
            .iter()
            .any(|message| message["role"] == "tool")
    );
}

pub(crate) fn a_goal_with_no_check_and_no_score_runs_until_the_judge_says_done() {
    let session = Session::new();
    std::fs::write(session.work.path().join("parts.txt"), "none\n").expect("write the fixture");
    commit_all(&session);
    let mut steps = Vec::new();
    for _ in 0..8 {
        steps.push(Step::Text("one more part is written"));
        steps.push(Step::Text(
            r#"{"verdict": "continue", "reason": "more parts remain"}"#,
        ));
    }
    steps.push(Step::Text("every part is written"));
    steps.push(Step::Text(
        r#"{"verdict": "done", "reason": "every part is written"}"#,
    ));
    let model = Model::start(steps);
    let result = session.run(&model, "", &["--goal", "write every part", "--json"]);
    assert_eq!(result.code, Some(0), "{}", result.stdout);
    assert!(
        !result.stdout.contains("no new measurement"),
        "{}",
        result.stdout
    );
}

pub(crate) fn a_goal_met_without_tools_in_a_repository_passes() {
    let session = Session::new();
    std::fs::write(session.work.path().join("notes.txt"), "present\n").expect("write the fixture");
    with_check(&session, "exit 0");
    commit_all(&session);
    let model = Model::start(vec![
        Step::Text("already done"),
        Step::Text(r#"{"verdict": "done", "reason": "the check passed"}"#),
    ]);
    let budget = crate::support::slow(20).to_string();
    let result = session.run(
        &model,
        "",
        &["--goal", "fix it", "--max-secs", &budget, "--json"],
    );
    assert_eq!(result.code, Some(0), "{}", result.stdout);
}

pub(crate) fn a_goal_run_whose_output_lands_in_the_workspace_still_passes() {
    let session = Session::new();
    let work = session.files.path();
    let git = Command::new("git")
        .args(["init", "-q"])
        .current_dir(work)
        .status()
        .expect("git init");
    assert!(git.success());
    std::fs::create_dir_all(work.join(".summonr")).expect("check directory");
    std::fs::write(work.join(".summonr/check.toml"), "check = 'exit 0'\n")
        .expect("write the check");
    let model = Model::start(vec![
        Step::Text("fixed"),
        Step::Text(r#"{"verdict": "done", "reason": "the check passed"}"#),
    ]);
    let result = session.run_with(
        &model,
        "",
        &["--goal", "fix it", "--json"],
        &Options {
            work: Some(work),
            ..Options::default()
        },
    );
    assert_eq!(result.code, Some(0), "{}", result.stdout);
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn a_failing_grep_check_does_not_complete_a_goal() {
    let session = Session::new();
    let work = session.files.path();
    std::fs::write(work.join("notes.txt"), "present\n").expect("write the fixture");
    std::fs::create_dir_all(work.join(".summonr")).expect("check directory");
    std::fs::write(
        work.join(".summonr/check.toml"),
        "check = 'grep missing notes.txt'\n",
    )
    .expect("write the check");
    let model = Model::start(vec![
        Step::Text("fixed"),
        Step::Text(r#"{"verdict": "done", "reason": "the check passed"}"#),
    ]);
    let budget = crate::support::slow(20).to_string();
    let result = session.run_with(
        &model,
        "",
        &["--goal", "fix it", "--max-secs", &budget, "--json"],
        &Options {
            work: Some(work),
            ..Options::default()
        },
    );
    assert_ne!(result.code, Some(0), "{}", result.stdout);
    assert!(
        !events(&result.stdout)
            .iter()
            .any(|event| event["type"] == "verify" && event["passed"] == true),
        "{}",
        result.stdout
    );
}

pub(crate) fn a_print_run_offers_no_task_list_and_a_goal_run_does() {
    let session = Session::new();
    let print = Model::start(vec![Step::Text("hi")]);
    let result = session.run(&print, "", &["--json", "--no-verify"]);
    assert_eq!(result.code, Some(0), "{}", result.stdout);
    assert_eq!(
        advertised(&print.requests()[0]),
        ["read", "search", "edit", "write", "run_command"]
    );
    with_check(&session, "exit 0");
    commit_all(&session);
    let goal = Model::start(vec![
        Step::Text("already done"),
        Step::Text(r#"{"verdict": "done", "reason": "the check passed"}"#),
    ]);
    let budget = crate::support::slow(20).to_string();
    let result = session.run(
        &goal,
        "",
        &["--goal", "fix it", "--max-secs", &budget, "--json"],
    );
    assert_eq!(result.code, Some(0), "{}", result.stdout);
    assert!(advertised(&goal.requests()[0]).contains(&"todo".to_string()));
}

pub(crate) fn a_print_run_sends_a_lean_first_request() {
    let session = Session::new();
    let model = Model::start(vec![Step::Text("hi")]);
    let result = session.run(&model, "", &["--json", "--no-verify"]);
    assert_eq!(result.code, Some(0), "{}", result.stdout);
    let request = &model.requests()[0];
    let system = request["messages"][0]["content"]
        .as_str()
        .unwrap_or_default();
    let tools = serde_json::to_string(&request["tools"]).expect("tool schemas");
    assert!(
        system.len() + tools.len() <= 6_400,
        "system {} bytes, tools {} bytes:\n{system}",
        system.len(),
        tools.len()
    );
}

pub(crate) fn a_session_header_names_the_build_that_wrote_it() {
    let session = Session::new();
    let result = session.run(&Model::start(vec![Step::Text("hi")]), "", &["--json"]);
    assert_eq!(result.code, Some(0), "{}", result.stdout);
    let (_, version) = session.cli(&["--version"]);
    let revision = version
        .split_once('(')
        .and_then(|(_, rest)| rest.split_once(')'))
        .map(|(revision, _)| revision.to_string())
        .expect("a revision in --version");
    let journal = std::fs::read_dir(session.home.path().join("sessions"))
        .expect("sessions dir")
        .flatten()
        .map(|entry| entry.path().join("session.jsonl"))
        .find(|path| path.exists())
        .expect("a journal");
    let header: Value = serde_json::from_str(
        std::fs::read_to_string(journal)
            .expect("journal text")
            .lines()
            .next()
            .expect("header"),
    )
    .expect("header json");
    assert_eq!(header["revision"], Value::String(revision));
}

pub(crate) fn a_print_run_says_its_check_was_not_run() {
    let session = Session::new();
    with_check(&session, "exit 0");
    let result = session.run(&Model::start(vec![Step::Text("hi")]), "", &["--json"]);
    assert_eq!(result.code, Some(0), "{}", result.stdout);
    let receipt = events(&result.stdout)
        .into_iter()
        .find(|event| event["type"] == "receipt")
        .unwrap_or_else(|| panic!("{}", result.stdout));
    assert_eq!(receipt["receipt"]["checks"], json!([]), "{receipt}");
    assert_eq!(receipt["receipt"]["verified"], false, "{receipt}");
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
        Duration::from_secs(crate::support::slow(6)),
        Box::new(Step::Text("too late")),
    )]);
    let result = session.run(
        &model,
        "",
        &[
            "--goal",
            "finish the task",
            "--max-secs",
            &crate::support::slow(3).to_string(),
            "--json",
        ],
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

pub(crate) fn a_session_resumed_by_id_from_another_folder_runs_in_its_own_workspace() {
    let session = Session::new();
    let first = session.run(&Model::start(vec![Step::Text("done")]), "", &["--json"]);
    assert_eq!(first.code, Some(0), "{}", first.stdout);
    let id = events(&first.stdout)[0]["id"]
        .as_str()
        .expect("session id")
        .to_string();
    let elsewhere = tempfile::tempdir().expect("another folder");
    let model = Model::start(vec![
        Step::Call("run_command", json!({"command": "pwd"})),
        Step::Text("ok"),
    ]);
    let resumed = session.run_with(
        &model,
        "",
        &["--session", &id, "--json"],
        &Options {
            work: Some(elsewhere.path()),
            ..Options::default()
        },
    );
    assert_eq!(resumed.code, Some(0), "{}", resumed.stdout);
    let workspace = dunce::canonicalize(session.work.path()).expect("workspace");
    let shown = json!(workspace.to_string_lossy()).to_string();
    assert!(
        model.tool_results()[0].contains(shown.trim_matches('"')),
        "{:?}",
        model.tool_results()
    );
}
