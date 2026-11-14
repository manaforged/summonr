use super::*;

#[test]
fn a_manifest_naming_the_default_hooks_file_loads_each_hook_once() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".claude-plugin")).unwrap();
    std::fs::create_dir_all(dir.path().join("hooks")).unwrap();
    std::fs::write(
        dir.path().join(MANIFEST),
        r#"{"name": "demo", "hooks": "./hooks/hooks.json"}"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("hooks/hooks.json"),
        r#"{"hooks": {"PreToolUse": [{"hooks": [{"type": "command", "command": "true"}]}]}}"#,
    )
    .unwrap();
    let plugin = Plugin::at("demo".to_string(), dir.path().to_path_buf());
    assert_eq!(plugin.hooks().of(HookEvent::PreTool).len(), 1);
}

#[test]
fn install_reports_the_hillclimb_skill_command_and_agent() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/hillclimb");
    let installed = super::manage::Plugin::install(&src).expect("install hillclimb");
    let report = installed.report();
    assert!(report.contains("skills 1"), "{report}");
    assert!(report.contains("commands climb"), "{report}");
    assert!(report.contains("agents climb"), "{report}");
    let bodies = [
        "skills/hillclimb/SKILL.md",
        "commands/climb.md",
        "agents/climb.md",
    ]
    .map(|path| procedure(&src.join(path)));
    assert_eq!(bodies[0], bodies[1]);
    assert_eq!(bodies[0], bodies[2]);
    for body in &bodies {
        assert!(body.contains("That is the baseline."), "{body}");
        assert!(
            body.contains("Report only the metric, the number"),
            "{body}"
        );
        for gate in ["10%", "percent", "do not build", "predict", "too small"] {
            assert!(!body.to_lowercase().contains(gate), "{gate} in {body}");
        }
    }
}

#[test]
fn install_reports_the_researchr_mode() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugins/researchr");
    let installed = super::manage::Plugin::install(&src).expect("install researchr");
    let report = installed.report();
    assert!(report.contains("skills 1"), "{report}");
    assert!(report.contains("commands research"), "{report}");
    assert!(report.contains("bin"), "{report}");
    let workspace = tempfile::tempdir().expect("test workspace");
    let command =
        commands::find("/researchr:research", workspace.path()).expect("installed command");
    assert!(command.plugin.hooks().of(HookEvent::PostTool).is_empty());
    let spec = ".summonr/research/spec-node3.toml";
    let expanded = commands::expand(
        &format!("/researchr:research {spec}"),
        workspace.path(),
        None,
        None,
    )
    .expect("expand the research command")
    .expect("the research command expands")
    .written();
    assert!(expanded.contains(spec), "{expanded}");
    let rendered = command
        .render("spec.toml", workspace.path(), None)
        .written();
    assert!(rendered.contains("researchr climb"), "{rendered}");
    assert!(rendered.contains("spec.toml"), "{rendered}");
    let data = [
        serde_json::json!({"schema": 1, "session": "fixture", "source": "session", "sequence": 1,
                           "record": {"kind": "assistant", "reasoning": "provider summary", "text": "answer"}}),
        serde_json::json!({"schema": 1, "session": "fixture", "source": "export", "sequence": 1,
                           "record": {"kind": "complete_prefix", "records": 1}}),
    ];
    let export = workspace.path().join("session.jsonl");
    std::fs::write(
        &export,
        data.iter()
            .map(|record| format!("{record}\n"))
            .collect::<String>(),
    )
    .expect("audit fixture");
    let db = workspace.path().join("audit.sqlite");
    let imported = researchr_cli(
        &command.plugin,
        &db,
        &["import", "--file", export.to_str().expect("export path")],
    );
    assert_eq!(imported["complete_export"], true);
    let saved = researchr_cli(
        &command.plugin,
        &db,
        &["audit", "--session", "fixture", "--full"],
    );
    let records = saved["events"].as_array().expect("saved events");
    assert_eq!(records.len(), 2);
    assert!(
        records
            .iter()
            .any(|record| record["record"]["record"]["reasoning"] == "provider summary")
    );
}

fn researchr_cli(plugin: &Plugin, db: &std::path::Path, args: &[&str]) -> Value {
    let output = std::process::Command::new("python3")
        .arg(plugin.dir.join("bin/researchr"))
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .expect("run installed researchr executable");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8(output.stderr).expect("UTF-8 CLI error")
    );
    serde_json::from_slice(&output.stdout).expect("CLI JSON output")
}

fn procedure(path: &std::path::Path) -> String {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    text.split_once("The change stays")
        .map(|(_, rest)| format!("The change stays{rest}"))
        .unwrap_or_else(|| panic!("{} has no procedure", path.display()))
}

fn tree(files: &[(&str, &str)]) -> (tempfile::TempDir, Plugin) {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let plugin = Plugin::at("demo".to_string(), dir.path().to_path_buf());
    (dir, plugin)
}

#[test]
fn skills_accept_the_root_and_paths_without_the_dot_prefix() {
    let (dir, plugin) = tree(&[
        (
            MANIFEST,
            r#"{"name": "demo", "skills": [".", "extra", "../outside"]}"#,
        ),
        ("SKILL.md", "---\nname: demo\n---\nBody."),
        ("extra/one/SKILL.md", "---\nname: one\n---\nBody."),
    ]);
    assert_eq!(
        plugin.skill_dirs(),
        [dir.path().to_path_buf(), dir.path().join("extra")]
    );
    assert_eq!(
        plugin.declared("skills").unwrap()[2],
        Source::Outside("../outside".to_string())
    );
}

#[test]
fn a_hooks_array_loads_files_and_inline_maps() {
    let command = |text: &str| {
        let handler = serde_json::json!({"type": "command", "command": text});
        serde_json::json!({"PreToolUse": [{"matcher": "Bash", "hooks": [handler]}]})
    };
    let manifest =
        serde_json::json!({"name": "demo", "hooks": ["./extra.json", command("inline")]});
    let (_dir, plugin) = tree(&[
        (MANIFEST, &manifest.to_string()),
        (
            "extra.json",
            &serde_json::json!({"hooks": command("file")}).to_string(),
        ),
    ]);
    let hooks = plugin.hooks();
    let commands: Vec<&str> = hooks
        .of(HookEvent::PreTool)
        .iter()
        .map(|hook| hook.command.as_str())
        .collect();
    assert_eq!(commands, ["file", "inline"]);
}

#[test]
fn mcp_servers_merge_in_order_and_take_the_documented_name() {
    let manifest = serde_json::json!({"name": "demo", "mcpServers": [
        "./servers.json",
        {"db": {"command": "inline-db"}},
        "./bundle.mcpb"
    ]});
    let (_dir, plugin) = tree(&[
        (MANIFEST, &manifest.to_string()),
        (
            ".mcp.json",
            r#"{"mcpServers": {"db": {"command": "default-db"}, "api": {"command": "api"}}}"#,
        ),
        (
            "servers.json",
            r#"{"mcpServers": {"db": {"command": "file-db"}}}"#,
        ),
    ]);
    let servers: BTreeMap<String, Value> = plugin
        .mcp_servers()
        .into_iter()
        .map(|server| (server.key, server.config))
        .collect();
    assert_eq!(servers["db"]["command"], "inline-db");
    assert_eq!(servers["api"]["command"], "api");
    assert_eq!(plugin.mcp_name("db"), "plugin_demo_db");
    assert_eq!(plugin.mcp_name("my.server"), "plugin_demo_my_server");
    assert_eq!(plugin.legacy_mcp_name("db"), "demo-db");
}

#[test]
fn a_claude_entry_reads_its_type_helper_timeout_and_endpoint() {
    let parse = |text: &str| serde_json::from_str::<summonr_types::mcp::Entry>(text).unwrap();
    let entry = parse(
        r#"{"type": "streamable-http", "url": "HTTPS://Mcp.Example.com:443/mcp/", "headersHelper": "print-headers", "timeout": 5000}"#,
    );
    assert_eq!(entry.transport, Some(summonr_types::mcp::Transport::Http));
    assert_eq!(entry.headers_helper.as_deref(), Some("print-headers"));
    assert_eq!(entry.timeout, Some(5000));
    let same = parse(r#"{"type": "http", "url": "https://mcp.example.com/mcp"}"#);
    assert_eq!(entry.endpoint(), same.endpoint());
    let other = parse(r#"{"type": "http", "url": "https://mcp.example.com:8443/mcp"}"#);
    assert_ne!(entry.endpoint(), other.endpoint());
    serde_json::from_str::<summonr_types::mcp::Entry>(r#"{"type": "carrier-pigeon"}"#)
        .expect_err("an unknown transport type is refused");
}

#[test]
fn an_unset_variable_without_a_default_stays_as_written_for_mcp() {
    let none = BTreeMap::new();
    let text =
        "${SUMMONR_PROBE_NEVER_SET}|${SUMMONR_PROBE_NEVER_SET:-fallback}|${user_config.missing}";
    let kept = Vars {
        user_config: Some(&none),
        env: Some(Unset::Kept),
        ..Vars::default()
    };
    assert_eq!(expand(text, &kept), "${SUMMONR_PROBE_NEVER_SET}|fallback|");
    let empty = Vars {
        user_config: Some(&none),
        env: Some(Unset::Empty),
        ..Vars::default()
    };
    assert_eq!(expand(text, &empty), "|fallback|");
}

#[test]
fn plugin_variables_expand_without_user_config_and_content_keeps_env_text() {
    let (dir, plugin) = tree(&[(MANIFEST, r#"{"name": "demo"}"#)]);
    let workspace = tempfile::tempdir().expect("workspace");
    let text = "${CLAUDE_PLUGIN_ROOT}|${SUMMONR_PROBE_NEVER_SET:-fallback}";
    let env = Vars {
        env: Some(Unset::Kept),
        ..plugin.vars(workspace.path())
    };
    let root = dir.path().display();
    assert_eq!(expand(text, &env), format!("{root}|fallback"));
    let content = plugin.vars(workspace.path());
    assert_eq!(
        expand(text, &content),
        format!("{root}|${{SUMMONR_PROBE_NEVER_SET:-fallback}}")
    );
}

#[test]
fn a_server_entry_of_the_wrong_shape_names_its_file_and_key() {
    let (dir, plugin) = tree(&[
        (MANIFEST, r#"{"name": "demo"}"#),
        (
            ".mcp.json",
            r#"{"mcpServers": {"good": {"command": "x"}, "bad": {"command": "x", "args": "--stdio"}}}"#,
        ),
    ]);
    let [bad, good] = plugin.mcp_servers().try_into().expect("two servers");
    assert_eq!(good.key, "good");
    let error = server_entry::<summonr_types::mcp::Entry, _>(&bad.file, &bad.key, bad.config)
        .expect_err("a string args is refused");
    let file = dir.path().join(".mcp.json");
    assert!(
        error.starts_with(&format!("{}: bad: ", file.display())),
        "{error}"
    );
}
