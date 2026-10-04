use std::time::Duration;

use serde_json::json;

use crate::support::{Model, Session, Step, advertised, calls};

pub(crate) fn the_client_offers_the_current_spec_with_roots() {
    let session = Session::new();
    session.run(
        &Model::start(vec![Step::Text("ok")]),
        &session.server("fixture1", ""),
        &[],
    );
    let received = session.received("fixture1");
    let hello = &calls(&received, "initialize")[0]["params"];
    assert_eq!(hello["protocolVersion"], json!("2025-11-25"), "{hello}");
    assert!(hello["capabilities"]["roots"].is_object(), "{hello}");
}

pub(crate) fn a_server_on_an_unknown_version_is_refused() {
    let session = Session::new();
    let config = session.server_env("fixture1", ", MCP_FIXTURE_VERSION = '1999-01-01'", "");
    let model = Model::start(vec![Step::Text("ok")]);
    let outcome = session.run(&model, &config, &["--json"]);
    assert!(
        outcome.stdout.contains("speaks MCP 1999-01-01"),
        "{}",
        outcome.stdout
    );
    let tools = advertised(&model.requests()[0]);
    assert!(
        !tools.iter().any(|tool| tool.starts_with("fixture1__")),
        "{tools:?}"
    );
}

pub(crate) fn the_server_can_ask_for_the_workspace_roots() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__roots", json!({})),
        Step::Text("ok"),
    ]);
    session.run(&model, &session.server("fixture1", ""), &[]);
    let result = &model.tool_results()[0];
    let workspace = session.work.path().canonicalize().unwrap();
    let name = workspace.file_name().unwrap().to_string_lossy().to_string();
    assert!(result.contains("file://"), "{result}");
    assert!(result.contains(&name), "{result}");
}

pub(crate) fn a_changed_tool_list_is_fetched_again() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__change", json!({})),
        Step::Text("ok"),
    ]);
    session.run(&model, &session.server("fixture1", ""), &[]);
    assert!(
        model.tool_results()[0].contains("changed its tool list"),
        "{:?}",
        model.tool_results()
    );
    let received = session.received("fixture1");
    let call = received
        .iter()
        .position(|frame| frame["method"] == "tools/call")
        .unwrap();
    assert!(
        received[call..]
            .iter()
            .any(|frame| frame["method"] == "tools/list"),
        "{received:?}"
    );
}

pub(crate) fn progress_reaches_the_event_stream() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__progress", json!({"ms": 0})),
        Step::Text("ok"),
    ]);
    let outcome = session.run(&model, &session.server("fixture1", ""), &["--json"]);
    let shown = outcome
        .stdout
        .lines()
        .filter(|line| line.contains("\"type\":\"progress\""))
        .count();
    assert_eq!(shown, 4, "{}", outcome.stdout);
    assert!(outcome.stdout.contains("step 4"), "{}", outcome.stdout);
}

pub(crate) fn progress_keeps_a_slow_call_alive() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__progress", json!({"ms": 600})),
        Step::Text("ok"),
    ]);
    session.run(&model, &session.server("fixture1", "timeout_secs = 1"), &[]);
    assert!(
        model.tool_results()[0].contains("progressed"),
        "{:?}",
        model.tool_results()
    );
}

pub(crate) fn server_log_messages_go_to_the_log_file() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__log", json!({})),
        Step::Text("ok"),
    ]);
    session.run(&model, &session.server("fixture1", ""), &[]);
    let log = std::fs::read_to_string(session.home.path().join("logs").join("mcp.log"))
        .unwrap_or_default();
    assert!(
        log.contains("fixture1/fixture warning \"disk low\""),
        "{log}"
    );
}

pub(crate) fn an_aborted_call_is_cancelled() {
    let session = Session::new();
    let model = Model::start(vec![Step::Call("fixture1__hang", json!({}))]);
    session.run(
        &model,
        &session.server("fixture1", ""),
        &["--max-secs", "5"],
    );
    drop(
        session.received_until("fixture1", Duration::from_secs(5), |received| {
            !calls(received, "notifications/cancelled").is_empty()
        }),
    );
    crate::support::pause(Duration::from_millis(200));
    let received = session.received("fixture1");
    let call = calls(&received, "tools/call")[0]["id"].clone();
    let cancelled = calls(&received, "notifications/cancelled");
    assert_eq!(cancelled.len(), 1, "{received:?}");
    assert_eq!(cancelled[0]["params"]["requestId"], call);
}
