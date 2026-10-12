use crate::support::{Model, Session, Step, fixture};

pub(crate) fn servers_are_added_listed_described_and_removed_from_the_shell() {
    let session = Session::new();
    let fixture = fixture().display().to_string();
    let (added, text) = session.cli(&[
        "mcp",
        "add",
        "fixture1",
        "--env",
        "MCP_FIXTURE_SERVE=1",
        "--env",
        "MCP_FIXTURE_SECRET=proof-secret-value",
        "--",
        &fixture,
    ]);
    assert!(
        added && text.contains("fixture1: ") && text.contains(" tools"),
        "{text}"
    );
    let (_, listed) = session.cli(&["mcp", "list"]);
    assert!(
        listed.contains("fixture1") && listed.contains("cached"),
        "{listed}"
    );
    let (ok, broken) = session.cli(&["mcp", "add", "demo", "--", "/usr/bin/true"]);
    assert!(!ok && broken.contains("demo: not reachable"), "{broken}");
    assert!(!broken.contains("configuration error"), "{broken}");
    let (_, listed) = session.cli(&["mcp", "list"]);
    let demo: Vec<&str> = listed
        .lines()
        .skip_while(|line| !line.starts_with("demo "))
        .take(2)
        .collect();
    assert!(
        demo.len() == 2 && demo[1].trim_start().starts_with("failed: "),
        "{listed}"
    );
    assert!(!demo[1].contains('…'), "{listed}");
    session.cli(&["mcp", "remove", "demo"]);
    assert!(
        cached(&session, "failed").is_empty(),
        "the failure marker outlived its server"
    );
    let config = std::fs::read_to_string(session.home.path().join("config.toml")).unwrap();
    session.run(&Model::start(vec![Step::Text("ok")]), &config, &[]);
    let (_, listed) = session.cli(&["mcp", "list"]);
    assert!(
        listed.contains("cached") && listed.contains(" tools "),
        "{listed}"
    );
    let (_, described) = session.cli(&["mcp", "get", "fixture1"]);
    assert!(described.lines().any(|line| line == "echo"), "{described}");
    assert!(described.contains("MCP_FIXTURE_SECRET"), "{described}");
    assert!(!described.contains("proof-secret-value"), "{described}");
    let elsewhere = std::process::Command::new(env!("CARGO_BIN_EXE_summonr"))
        .args(["mcp", "remove", "fixture1"])
        .current_dir(session.home.path())
        .env("HOME", session.home.path())
        .env("USERPROFILE", session.home.path())
        .env("SUMMONR_HOME", session.home.path())
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(elsewhere.status.success(), "{elsewhere:?}");
    let (_, listed) = session.cli(&["mcp", "list"]);
    assert!(listed.contains("no MCP servers configured"), "{listed}");
    assert!(
        cached(&session, "json").is_empty(),
        "the tool list outlived its server"
    );
}

fn cached(session: &Session, extension: &str) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(session.home.path().join("mcp"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|found| found == extension))
        .collect()
}
