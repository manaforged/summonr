use serde_json::json;

use crate::support::{Model, Session, Step, advertised};

pub(crate) fn tools_core_calls_return_results_to_the_model() {
    let session = Session::new();
    let work = session.work.path();
    std::fs::create_dir_all(work.join(".summonr/skills")).expect("skill directory");
    std::fs::write(
        work.join(".summonr/skills/probe.md"),
        "# Tool probe\nRead note.txt.\n",
    )
    .expect("skill body");
    let model = Model::start(vec![
        Step::Call("write", json!({"path": "note.txt", "content": "before\n"})),
        Step::Call(
            "edit",
            json!({"path": "note.txt", "old": "before", "new": "after"}),
        ),
        Step::Call("read", json!({"path": "note.txt"})),
        Step::Call("search", json!({"pattern": "after", "glob": "note.txt"})),
        Step::Call("run_command", json!({"command": "echo harness-ready"})),
        Step::Call("skill", json!({"name": "probe"})),
        Step::Text("done"),
    ]);
    let outcome = session.run(&model, "", &["--no-verify", "--json"]);
    assert_eq!(outcome.code, Some(0), "{}", outcome.stdout);
    let requests = model.requests();
    let offered = advertised(&requests[0]);
    for name in ["read", "search", "edit", "write", "run_command", "skill"] {
        assert!(offered.iter().any(|tool| tool == name), "{offered:?}");
    }
    let results = model.tool_results();
    assert_eq!(results.len(), 6, "{results:?}");
    assert_eq!(
        std::fs::read_to_string(work.join("note.txt")).expect("edited file"),
        "after\n"
    );
    assert!(results[2].contains("1:after"), "{results:?}");
    assert!(results[3].contains("note.txt:1:1:after"), "{results:?}");
    assert!(
        results[4].contains("exit 0") && results[4].contains("harness-ready"),
        "{results:?}"
    );
    assert!(results[5].contains("Read note.txt."), "{results:?}");
    let failed = outcome.stdout.lines().any(|line| {
        let event: serde_json::Value = serde_json::from_str(line).expect("JSON event");
        event["type"] == "tool_result" && event["success"] == false
    });
    assert!(!failed, "{}", outcome.stdout);
}

pub(crate) fn tools_module_configuration_errors_are_reported() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("write", json!({"path": "unexpected.txt", "content": "bad"})),
        Step::Text("done"),
    ]);
    let outcome = session.run(
        &model,
        "[modules]\nbrowser = true\n[browser]\nprofile = '../invalid'\n",
        &["--no-verify"],
    );
    let stderr = crate::support::read(&session.file("stderr"));
    assert_eq!(outcome.code, Some(1), "{stderr}");
    assert!(
        stderr.contains("must be a plain directory name"),
        "{stderr}"
    );
    assert!(
        model.requests().is_empty(),
        "model called after failed setup"
    );
    assert!(!session.work.path().join("unexpected.txt").exists());
}

pub(crate) fn tools_optional_modules_are_advertised_and_dispatch_calls() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("browser", json!({"action": "close"})),
        Step::Call("web", json!({"url": "https://127.0.0.1/"})),
        Step::Text("done"),
    ]);
    let outcome = session.run(
        &model,
        "[modules]\nsearch = true\nbrowser = true\n",
        &["--no-verify"],
    );
    assert_eq!(outcome.code, Some(0), "{}", outcome.stdout);
    let requests = model.requests();
    let offered = advertised(&requests[0]);
    assert!(offered.iter().any(|name| name == "web"), "{offered:?}");
    assert!(offered.iter().any(|name| name == "browser"), "{offered:?}");
    let results = model.tool_results();
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(results[0].contains("browser was not open"), "{results:?}");
    assert!(
        results[1].contains("that host is not allowed"),
        "{results:?}"
    );
}

pub(crate) fn tools_disabled_modules_are_hidden_and_refuse_calls() {
    let session = Session::new();
    let model = Model::start(vec![
        Step::Call("browser", json!({"action": "close"})),
        Step::Call("web", json!({"query": "Rust"})),
        Step::Text("done"),
    ]);
    let outcome = session.run(
        &model,
        "[modules]\nsearch = false\nbrowser = false\n",
        &["--no-verify"],
    );
    assert_eq!(outcome.code, Some(0), "{}", outcome.stdout);
    let requests = model.requests();
    let offered = advertised(&requests[0]);
    assert!(
        !offered
            .iter()
            .any(|name| name == "web" || name == "browser"),
        "{offered:?}"
    );
    assert!(offered.iter().any(|name| name == "search"), "{offered:?}");
    let results = model.tool_results();
    assert_eq!(results.len(), 2, "{results:?}");
    assert!(
        results[0].contains("tool `browser` is unavailable in the current mode"),
        "{results:?}"
    );
    assert!(
        results[1].contains("tool `web` is unavailable in the current mode"),
        "{results:?}"
    );
    assert!(!session.home.path().join("browser").exists());
}
