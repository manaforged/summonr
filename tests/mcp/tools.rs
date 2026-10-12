use serde_json::json;

use crate::support::{Model, Options, Session, Step, Wire, advertised};

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

pub(crate) fn tools_results_and_failures_round_trip_on_every_wire() {
    for wire in [
        Wire::Completions,
        Wire::Anthropic,
        Wire::Responses,
        Wire::Gemini,
    ] {
        let session = Session::new();
        let model = Model::start(vec![
            Step::Call("read", json!({"path": "absent.txt"})),
            Step::Call(
                "write",
                json!({"path": "note.txt", "content": "recovered\n"}),
            ),
            Step::Call("read", json!({"path": "note.txt"})),
            Step::Text("done"),
        ]);
        let outcome = session.run_with(
            &model,
            "",
            &["--no-verify", "--json"],
            &Options {
                wire,
                ..Options::default()
            },
        );
        assert_eq!(outcome.code, Some(0), "{wire:?}: {}", outcome.stdout);
        let requests = model.requests();
        assert_eq!(requests.len(), 4, "{wire:?}: {requests:?}");
        let offered = match wire {
            Wire::Completions => &requests[0]["tools"][0]["function"]["name"],
            Wire::Anthropic | Wire::Responses => &requests[0]["tools"][0]["name"],
            Wire::Gemini => &requests[0]["tools"][0]["functionDeclarations"][0]["name"],
        };
        assert_eq!(offered, "read", "{wire:?}: {requests:?}");
        let returned = match wire {
            Wire::Completions => requests[3]["messages"].as_array(),
            Wire::Anthropic => requests[3]["messages"].as_array(),
            Wire::Responses => requests[3]["input"].as_array(),
            Wire::Gemini => requests[3]["contents"].as_array(),
        }
        .expect("sent history");
        let last = returned.last().expect("sent tool result").to_string();
        assert!(last.contains("1:recovered"), "{wire:?}: {last}");
        let events: Vec<serde_json::Value> = outcome
            .stdout
            .lines()
            .map(|line| serde_json::from_str(line).expect("JSON event"))
            .filter(|event: &serde_json::Value| event["type"] == "tool_result")
            .collect();
        assert_eq!(events.len(), 3, "{wire:?}: {events:?}");
        assert_eq!(events[0]["success"], false, "{wire:?}: {events:?}");
        assert_eq!(events[1]["success"], true, "{wire:?}: {events:?}");
        assert_eq!(events[2]["success"], true, "{wire:?}: {events:?}");
        assert_eq!(
            std::fs::read_to_string(session.work.path().join("note.txt")).expect("written file"),
            "recovered\n"
        );
    }
}

pub(crate) fn tools_model_fixture_stops_listening_when_dropped() {
    let model = Model::start(Vec::new());
    let retained = std::sync::Arc::downgrade(&model.requests);
    drop(model);
    assert!(
        retained.upgrade().is_none(),
        "fixture listener or workers retained state"
    );
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
    let browser = requests[0]["tools"]
        .as_array()
        .expect("tool schemas")
        .iter()
        .find(|tool| tool["function"]["name"] == "browser")
        .expect("browser schema");
    let properties = &browser["function"]["parameters"]["properties"];
    for (group, field, kind) in [
        ("by", "value", "array"),
        ("by", "label", "array"),
        ("by", "index", "array"),
        ("for", "selector", "string"),
        ("for", "text", "string"),
        ("for", "ms", "integer"),
        ("options", "button", "string"),
        ("options", "count", "integer"),
        ("options", "modifiers", "array"),
        ("options", "position", "array"),
    ] {
        assert_eq!(
            properties[group]["properties"][field]["type"], kind,
            "{group}.{field}"
        );
        assert_eq!(properties[group]["additionalProperties"], false, "{group}");
    }
    assert_eq!(
        properties["by"]["properties"]["value"]["items"]["type"],
        "string"
    );
    assert_eq!(
        properties["by"]["properties"]["index"]["items"]["minimum"],
        0
    );
    assert_eq!(
        properties["options"]["properties"]["position"]["minItems"],
        2
    );
    assert_eq!(
        properties["options"]["properties"]["position"]["maxItems"],
        2
    );
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
