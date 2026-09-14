use super::*;

#[test]
fn usage_rejects_unknown_words() {
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let error = runtime
        .block_on(run(&["dance".to_string()], Path::new(".")))
        .expect_err("usage");
    assert!(error.to_string().contains("summonr room serve"));
}

#[test]
fn setup_claude_merges_the_mcp_entry_and_installs_the_skill() {
    let dir = std::env::temp_dir().join(format!("summon-room-setup-{}", std::process::id()));
    let claude = dir.join("claude");
    let project = dir.join("project");
    std::fs::create_dir_all(&project).expect("project");
    std::fs::write(
        project.join(".mcp.json"),
        r#"{"mcpServers":{"other":{"command":"x"}}}"#,
    )
    .expect("seed");
    let report = setup_claude(&project, &claude).expect("setup");
    assert!(report.contains("written"));
    let config: Value =
        serde_json::from_str(&std::fs::read_to_string(project.join(".mcp.json")).expect("read"))
            .expect("json");
    assert_eq!(
        config
            .pointer("/mcpServers/other/command")
            .and_then(Value::as_str),
        Some("x")
    );
    assert_eq!(
        config.pointer("/mcpServers/summon-room/args"),
        Some(&json!(["room", "serve"]))
    );
    let skill = std::fs::read_to_string(claude.join("skills/summon-room/SKILL.md")).expect("skill");
    assert!(skill.starts_with("---\nname: summon-room"));
    let again = setup_claude(&project, &claude).expect("setup again");
    assert!(again.contains("unchanged"));
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn setup_codex_adds_the_server_table_and_keeps_the_rest() {
    let root = std::env::temp_dir().join(format!("summon-room-codex-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("root");
    std::fs::write(
        root.join("config.toml"),
        "model = \"gpt-5\"\n\n[mcp_servers.other]\ncommand = \"x\"\n",
    )
    .expect("seed");
    let report = setup_codex(&root).expect("setup");
    assert!(report.contains("written"));
    let text = std::fs::read_to_string(root.join("config.toml")).expect("read");
    assert!(text.contains("model = \"gpt-5\""));
    assert!(text.contains("[mcp_servers.other]"));
    assert!(text.contains("[mcp_servers.summon-room]"));
    assert!(text.contains("args = [\"room\", \"serve\"]"));
    assert!(root.join("skills/summon-room/SKILL.md").is_file());
    assert!(setup_codex(&root).expect("again").contains("unchanged"));
    drop(std::fs::remove_dir_all(&root));
}
