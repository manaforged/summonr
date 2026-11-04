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
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins/hillclimb");
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
fn install_reports_the_lanes_skill_and_command() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("plugins/lanes");
    let installed = super::manage::Plugin::install(&src).expect("install lanes");
    let report = installed.report();
    assert!(report.contains("skills 1"), "{report}");
    assert!(report.contains("commands lanes"), "{report}");
    assert!(!report.contains("agents"), "{report}");
    assert!(report.contains("bin"), "{report}");
    let workspace = tempfile::tempdir().expect("test workspace");
    let command = commands::find("/lanes:lanes", workspace.path()).expect("installed command");
    let skill = command.plugin.dir.join("skills/lanes/SKILL.md");
    let rendered = command.render("coordinate workers", workspace.path());
    assert!(
        rendered.contains(skill.to_str().expect("skill path")),
        "{rendered}"
    );
    assert!(rendered.contains("coordinate workers"), "{rendered}");
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
    let imported = lanes_cli(
        &command.plugin,
        &db,
        &["import", "--file", export.to_str().expect("export path")],
    );
    assert_eq!(imported["complete_export"], true);
    let saved = lanes_cli(
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

fn lanes_cli(plugin: &Plugin, db: &std::path::Path, args: &[&str]) -> Value {
    let output = std::process::Command::new("python3")
        .arg(plugin.dir.join("bin/lanes"))
        .arg("--db")
        .arg(db)
        .args(args)
        .output()
        .expect("run installed lanes executable");
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
