#![allow(
    dead_code,
    unused_imports,
    reason = "shares tests/support with other targets"
)]

mod support;

use std::path::Path;
use std::process::{Command, Output, Stdio};

use serde_json::{Value, json};
use support::{Model, Step};

fn summonr(home: &Path, port: u16, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_summonr"))
        .args(args)
        .env(
            "SUMMONR_MODELS_URL",
            format!("http://127.0.0.1:{port}/registry.json"),
        )
        .current_dir(home)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("SUMMONR_HOME", home)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

#[test]
fn a_leads_children_are_listed_from_its_journal_after_it_exits() {
    let home = tempfile::tempdir().unwrap();
    let model = Model::start(vec![
        Step::Call(
            "task",
            json!({
                "access": "research",
                "assignment": "read the README",
                "name": "reader",
                "workspace": "shared"
            }),
        ),
        Step::Text("the README says hi"),
        Step::Text("the reader is done"),
    ]);
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            "[endpoint.local]\nbase_url = \"http://127.0.0.1:{}/v1\"\nmodel = \"replay\"\napi_key = \"k\"\ncontext_window = 32768\n",
            model.port
        ),
    )
    .unwrap();
    let run = summonr(
        home.path(),
        model.port,
        &["-e", "local", "-p", "--json", "split the work"],
    );
    let events: Vec<Value> = String::from_utf8_lossy(&run.stdout)
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    let lead = events
        .iter()
        .find(|event| event["type"] == "session")
        .and_then(|event| event["id"].as_str())
        .unwrap_or_else(|| panic!("no session event: {}", String::from_utf8_lossy(&run.stderr)))
        .to_owned();
    assert_eq!(
        run.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );

    let listed = summonr(
        home.path(),
        model.port,
        &["session", "children", &lead, "--json"],
    );
    let stdout = String::from_utf8_lossy(&listed.stdout);
    assert!(
        listed.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&listed.stderr)
    );
    let spawned: Vec<&str> = events
        .iter()
        .filter(|event| event["type"] == "child" && event["event"]["type"] == "spawned")
        .filter_map(|event| event["id"].as_str())
        .collect();
    let children: Vec<Value> = serde_json::from_str(&stdout).unwrap();
    let seen: Vec<(&str, &str, &str, &str)> = children
        .iter()
        .map(|child| {
            (
                child["id"].as_str().unwrap_or_default(),
                child["name"].as_str().unwrap_or_default(),
                child["parent"].as_str().unwrap_or_default(),
                child["assignment"].as_str().unwrap_or_default(),
            )
        })
        .collect();
    let child = spawned.first().copied().unwrap_or_default();
    assert_eq!(
        seen,
        vec![(child, "reader", lead.as_str(), "read the README")],
        "{stdout}"
    );
}
