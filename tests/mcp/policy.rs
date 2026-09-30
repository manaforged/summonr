use serde_json::json;

use crate::support::{Model, Session, Step, advertised, calls};

pub(crate) fn strict_mode_denies_a_listed_tool_and_runs_an_allowed_one() {
    let session = Session::new();
    std::fs::write(
        session.home.path().join("policy.toml"),
        "deny = [\"fixture1__echo\"]\nallow = [\"fixture1__write\"]\n",
    )
    .unwrap();
    let model = Model::start(vec![
        Step::Call("fixture1__echo", json!({"text": "x"})),
        Step::Call("fixture1__write", json!({})),
        Step::Text("ok"),
    ]);
    session.run(&model, &session.server("fixture1", ""), &["--strict"]);
    let tools = advertised(&model.requests()[0]);
    assert!(!tools.contains(&"fixture1__echo".to_string()), "{tools:?}");
    let results = model.tool_results();
    assert!(results[0].contains("denied by policy"), "{results:?}");
    assert!(results[1].contains("wrote"), "{results:?}");
    let received = session.received("fixture1");
    let sent: Vec<_> = calls(&received, "tools/call")
        .iter()
        .map(|frame| frame["params"]["name"].clone())
        .collect();
    assert_eq!(sent, [json!("write")]);
}

pub(crate) fn strict_mode_without_an_allow_entry_refuses_a_tool_that_changes_state() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__write", json!({})),
        Step::Call("fixture1__echo", json!({"text": "read"})),
        Step::Text("ok"),
    ]);
    session.run(&model, &session.server("fixture1", ""), &["--strict"]);
    let results = model.tool_results();
    assert!(
        results[0].contains("not on every configured allow list"),
        "{results:?}"
    );
    assert!(results[1].contains("read"), "{results:?}");
    let received = session.received("fixture1");
    assert_eq!(calls(&received, "tools/call").len(), 1, "{received:?}");
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn a_command_allow_list_does_not_block_mcp_tools_outside_strict_mode() {
    let session = Session::new();
    std::fs::write(
        session.home.path().join("policy.toml"),
        "allow = [\"cargo\"]\n",
    )
    .unwrap();
    let model = Model::start(vec![
        Step::Call("fixture1__write", json!({})),
        Step::Text("ok"),
    ]);
    session.run(&model, &session.server("fixture1", ""), &["--sandbox"]);
    assert!(
        model.tool_results()[0].contains("wrote"),
        "{:?}",
        model.tool_results()
    );
}

pub(crate) fn tools_outside_the_allowlist_are_not_advertised() {
    let session = Session::new();
    let model = Model::start(vec![Step::Text("ok")]);
    session.run(
        &model,
        &session.server("fixture1", "tools = [\"echo\"]"),
        &[],
    );
    let tools = advertised(&model.requests()[0]);
    assert!(tools.contains(&"fixture1__echo".to_string()), "{tools:?}");
    assert!(!tools.contains(&"fixture1__write".to_string()), "{tools:?}");
}

pub(crate) fn an_unapproved_project_server_is_named() {
    let session = Session::new();
    project(&session);
    let outcome = session.run(&Model::start(vec![Step::Text("ok")]), "", &["--json"]);
    assert!(
        outcome.stdout.contains("wait for approval: proj"),
        "{}",
        outcome.stdout
    );
}

pub(crate) fn a_tuning_table_keeps_a_project_server_waiting() {
    let session = Session::new();
    project(&session);
    let config = "[mcp.proj]\ntools = [\"echo\"]\n";
    let outcome = session.run(&Model::start(vec![Step::Text("ok")]), config, &["--json"]);
    assert!(
        outcome.stdout.contains("wait for approval: proj"),
        "{}",
        outcome.stdout
    );
}

pub(crate) fn a_zero_timeout_is_refused() {
    let session = Session::new();
    let config = session.server("fixture1", "timeout_secs = 0");
    let outcome = session.run(&Model::start(vec![Step::Text("ok")]), &config, &["--json"]);
    assert!(
        outcome.stdout.contains("timeout_secs must be at least 1"),
        "{}",
        outcome.stdout
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn a_server_runs_inside_the_sandbox() {
    let session = Session::with_files(std::path::Path::new(env!("CARGO_TARGET_TMPDIR")));
    let model = Model::start(vec![
        Step::Call("fixture1__echo", json!({"text": "inside"})),
        Step::Text("ok"),
    ]);
    session.run(&model, &session.server("fixture1", ""), &["--sandbox"]);
    assert!(
        model.tool_results()[0].contains("inside"),
        "{:?}",
        model.tool_results()
    );
    assert!(
        !session.file("fixture1.log").exists(),
        "the server wrote outside the workspace"
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn a_server_restarts_when_the_sandbox_changes() {
    let session = Session::new();
    let outside = session.file("granted.txt");
    let model = Model::start(vec![
        Step::Call("fixture1__echo", json!({"text": "before"})),
        Step::Call(
            "write",
            json!({"path": outside.display().to_string(), "content": "x"}),
        ),
        Step::Call("fixture1__echo", json!({"text": "after"})),
        Step::Text("ok"),
    ]);
    session.run(&model, &session.server("fixture1", ""), &["--sandbox"]);
    let results = model.tool_results();
    assert!(results[2].contains("after"), "{results:?}");
    assert!(
        outside.exists(),
        "the write grant did not apply: {results:?}"
    );
    assert_eq!(
        session.starts("fixture1"),
        3,
        "list, first call, call after the grant"
    );
}

fn project(session: &Session) {
    let command = crate::support::fixture();
    std::fs::write(
        session.work.path().join(".mcp.json"),
        json!({"mcpServers": {"proj": {"command": command}}}).to_string(),
    )
    .unwrap();
}
