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
    let (_, listed) = session.cli(&["mcp", "list"]);
    assert!(
        listed.contains("demo") && listed.contains("failed: "),
        "{listed}"
    );
    session.cli(&["mcp", "remove", "demo"]);
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
    let (removed, text) = session.cli(&["mcp", "remove", "fixture1"]);
    assert!(removed, "{text}");
    let (_, listed) = session.cli(&["mcp", "list"]);
    assert!(listed.contains("no MCP servers configured"), "{listed}");
}
