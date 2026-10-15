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
    assert!(
        results[1].contains("not on every configured allow list"),
        "{results:?}"
    );
    let received = session.received("fixture1");
    assert!(calls(&received, "tools/call").is_empty(), "{received:?}");
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
    session.run(&model, &session.server("fixture1", ""), &[]);
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

fn project(session: &Session) {
    let command = crate::support::fixture();
    std::fs::write(
        session.work.path().join(".mcp.json"),
        json!({"mcpServers": {"proj": {"command": command}}}).to_string(),
    )
    .unwrap();
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn a_relocated_summonr_home_stays_protected_from_read_and_search() {
    let session = Session::new();
    let config = session.home.path().join("config.toml");
    let model = Model::start(vec![
        Step::Call("read", json!({"path": config.display().to_string()})),
        Step::Call(
            "search",
            json!({"pattern": "base_url", "path": session.home.path().display().to_string()}),
        ),
        Step::Text("done"),
    ]);
    session.run(&model, "", &["--strict", "--no-verify"]);
    let results = model.tool_results();
    assert!(results[0].contains("protected path"), "{results:?}");
    assert!(!results[1].contains("config.toml"), "{results:?}");
}
