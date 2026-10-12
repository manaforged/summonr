use std::time::Duration;

use serde_json::json;

use crate::support::{Model, Options, Session, Step, advertised, calls, repository};

pub(crate) fn a_configured_server_is_on_by_default_and_starts_only_when_called() {
    let session = Session::new();
    let config = session.server("fixture1", "");
    session.run(&Model::start(vec![Step::Text("ok")]), &config, &[]);
    assert_eq!(
        session.starts("fixture1"),
        1,
        "the first session lists tools once"
    );
    let model = Model::start(vec![Step::Text("ok")]);
    session.run(&model, &config, &[]);
    assert_eq!(
        session.starts("fixture1"),
        1,
        "a cached list needs no process"
    );
    let tools = advertised(&model.requests()[0]);
    assert!(tools.contains(&"fixture1__echo".to_string()), "{tools:?}");
}

pub(crate) fn an_idle_server_stops_and_starts_again_on_the_next_call() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__echo", json!({"text": "one"})),
        Step::After(
            Duration::from_secs(crate::support::slow(3)),
            Box::new(Step::Call("fixture1__echo", json!({"text": "two"}))),
        ),
        Step::Text("ok"),
    ]);
    let idle = format!("idle_secs = {}", crate::support::slow(1));
    session.run(&model, &session.server("fixture1", &idle), &[]);
    assert_eq!(
        session.starts("fixture1"),
        3,
        "list, first call, call after idle"
    );
    let results = model.tool_results();
    assert!(
        results[0].contains("one") && results[1].contains("two"),
        "{results:?}"
    );
}

pub(crate) fn the_least_recently_used_idle_server_stops_at_the_server_cap() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__echo", json!({"text": "one"})),
        Step::Call("fixture2__echo", json!({"text": "two"})),
        Step::Call("fixture1__echo", json!({"text": "three"})),
        Step::Text("ok"),
    ]);
    let config = format!(
        "{}{}[limits]\nservers_max = 1\n",
        session.server("fixture1", "idle_secs = 60"),
        session.server("fixture2", "idle_secs = 60"),
    );
    session.run(&model, &config, &[]);
    assert_eq!(
        session.starts("fixture1"),
        3,
        "list, first call, and a restart after the cap stopped it"
    );
    assert!(model.tool_results()[2].contains("three"));
}

pub(crate) fn keep_alive_servers_count_toward_the_server_cap() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("fixture1__echo", json!({"text": "one"})),
        Step::Call("fixture2__echo", json!({"text": "two"})),
        Step::Call("fixture1__echo", json!({"text": "three"})),
        Step::Text("ok"),
    ]);
    let config = format!(
        "{}{}[limits]\nservers_max = 1\n",
        session.server("fixture1", "keep_alive = true"),
        session.server("fixture2", "keep_alive = true"),
    );
    session.run(&model, &config, &[]);
    assert!(
        session.starts("fixture1") >= 2,
        "fixture1 restarted after the cap stopped it: {} starts",
        session.starts("fixture1")
    );
    assert!(model.tool_results()[2].contains("three"));
}

pub(crate) fn tool_lists_are_cached_per_workspace() {
    let session = Session::new();
    let echo_only = session.server_env("fixture1", ", MCP_FIXTURE_TOOLS = 'echo'", "");
    session.run(&Model::start(vec![Step::Text("ok")]), &echo_only, &[]);
    let other = repository();
    let write_only = session.server_env("fixture1", ", MCP_FIXTURE_TOOLS = 'write'", "");
    let model = Model::start(vec![Step::Text("ok")]);
    let options = Options {
        work: Some(other.path()),
        ..Options::default()
    };
    session.run_with(&model, &write_only, &[], &options);
    let tools = advertised(&model.requests()[0]);
    assert!(tools.contains(&"fixture1__write".to_string()), "{tools:?}");
}

pub(crate) fn an_empty_cached_tool_list_is_fetched_again() {
    let session = Session::new();
    let empty = session.server_env("fixture1", ", MCP_FIXTURE_TOOLS = 'none'", "");
    session.run(&Model::start(vec![Step::Text("ok")]), &empty, &[]);
    let model = Model::start(vec![Step::Text("ok")]);
    session.run(&model, &session.server("fixture1", ""), &[]);
    let tools = advertised(&model.requests()[0]);
    assert!(tools.contains(&"fixture1__echo".to_string()), "{tools:?}");
}

pub(crate) fn an_unchanged_tool_list_is_not_rewritten() {
    let session = Session::new();
    let config = session.server("fixture1", "");
    session.run(&Model::start(vec![Step::Text("ok")]), &config, &[]);
    let cache = std::fs::read_dir(session.home.path().join("mcp"))
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|ext| ext == "json"))
        .unwrap();
    let before = std::fs::metadata(&cache).unwrap().modified().unwrap();
    crate::support::pause(Duration::from_millis(50));
    let model = Model::start(vec![
        Step::Call("fixture1__echo", json!({"text": "x"})),
        Step::Text("ok"),
    ]);
    session.run(&model, &config, &[]);
    assert_eq!(session.starts("fixture1"), 2, "the call relisted the tools");
    let after = std::fs::metadata(&cache).unwrap().modified().unwrap();
    assert_eq!(before, after, "the cache was rewritten with the same list");
}

pub(crate) fn a_tool_list_that_never_ends_is_refused() {
    let session = Session::new();
    let config = session.server_env("fixture1", ", MCP_FIXTURE_ENDLESS = '1'", "");
    let model = Model::start(vec![Step::Text("ok")]);
    let outcome = session.run(&model, &config, &["--json"]);
    assert!(
        outcome.stdout.contains("more than 64 pages"),
        "{}",
        outcome.stdout
    );
    let tools = advertised(&model.requests()[0]);
    assert!(
        !tools.iter().any(|tool| tool.starts_with("fixture1__")),
        "{tools:?}"
    );
}

pub(crate) fn a_server_that_never_answers_is_given_up_at_its_timeout() {
    let session = Session::new();
    let secs = crate::support::slow(1);
    let timeout = format!("timeout_secs = {secs}");
    let config = session.server_env("fixture1", ", MCP_FIXTURE_MUTE = '1'", &timeout);
    let outcome = session.run(&Model::start(vec![Step::Text("ok")]), &config, &["--json"]);
    assert!(
        outcome
            .stdout
            .contains(&format!("`initialize` got no reply in {secs}s")),
        "{}",
        outcome.stdout
    );
    let received = session.received("fixture1");
    let methods: Vec<_> = received
        .iter()
        .map(|frame| frame["method"].clone())
        .collect();
    assert_eq!(methods, [json!("initialize")], "{received:?}");
    assert!(calls(&received, "notifications/cancelled").is_empty());
}

#[cfg(unix)]
pub(crate) fn a_server_behind_a_wrapper_leaves_no_process_behind() {
    let session = Session::new();
    let marker = format!("wrapped-{}", std::process::id());
    let model = Model::start(vec![
        Step::Call("fixture1__echo", json!({"text": "x"})),
        Step::After(Duration::from_secs(3), Box::new(Step::Text("ok"))),
    ]);
    session.run(&model, &wrapped(&session, &marker), &[]);
    assert!(
        model.tool_results()[0].contains('x'),
        "{:?}",
        model.tool_results()
    );
    let left = crate::support::survivors(&marker);
    assert!(left.is_empty(), "servers left running: {left:?}");
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
pub(crate) fn a_sandboxed_server_behind_a_wrapper_leaves_no_process_behind() {
    let session = Session::new();
    let marker = format!("sandboxed-{}", std::process::id());
    let model = Model::start(vec![
        Step::Call("fixture1__echo", json!({"text": "x"})),
        Step::After(Duration::from_secs(3), Box::new(Step::Text("ok"))),
    ]);
    session.run(&model, &wrapped(&session, &marker), &["--sandbox"]);
    assert!(
        model.tool_results()[0].contains('x'),
        "{:?}",
        model.tool_results()
    );
    let left = crate::support::survivors(&marker);
    assert!(left.is_empty(), "servers left running: {left:?}");
}

#[cfg(unix)]
pub(crate) fn ctrl_c_stops_the_servers() {
    let session = Session::new();
    let marker = format!("interrupted-{}", std::process::id());
    let model = Model::start(vec![Step::Call("fixture1__hang", json!({}))]);
    let log = session.file("fixture1.log");
    let options = Options {
        interrupt: Some((&log, "tools/call")),
        ..Options::default()
    };
    let outcome = session.run_with(&model, &wrapped(&session, &marker), &[], &options);
    assert_eq!(outcome.code, Some(130));
    let left = crate::support::survivors(&marker);
    assert!(left.is_empty(), "servers left running: {left:?}");
}

#[cfg(unix)]
fn wrapped(session: &Session, marker: &str) -> String {
    let launch = format!(
        "command = 'sh'\nargs = ['-c', '\"$0\" \"$1\"; true', '{}', '{marker}']",
        crate::support::fixture().display()
    );
    session.launched(
        "fixture1",
        &launch,
        ", MCP_FIXTURE_LINGER = '1'",
        "idle_secs = 1",
    )
}
