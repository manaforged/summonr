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
        assert!(body.contains("Report only the metric, the number"), "{body}");
        for gate in ["10%", "percent", "do not build", "predict", "too small"] {
            assert!(!body.to_lowercase().contains(gate), "{gate} in {body}");
        }
    }
}

fn procedure(path: &std::path::Path) -> String {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    text.split_once("The change stays")
        .map(|(_, rest)| format!("The change stays{rest}"))
        .unwrap_or_else(|| panic!("{} has no procedure", path.display()))
}
