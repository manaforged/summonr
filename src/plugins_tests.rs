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
    let lead = body(&src.join("commands/lanes.md"));
    assert_eq!(lead, body(&src.join("skills/lanes/SKILL.md")));
    assert!(lead.contains("| Lane agent | `climb` |"), "{lead}");
    assert!(lead.contains("| Start lane | `task`"), "{lead}");
    assert!(lead.contains("| Steer lane | `send`"), "{lead}");
    assert!(lead.contains("| Tick | `summonr cron add"), "{lead}");
    let blob = std::fs::read_to_string(src.join("commands/lanes.md")).expect("command");
    for banned in ["/Users/"] {
        assert!(
            !blob.to_lowercase().contains(&banned.to_lowercase()),
            "{banned}"
        );
    }
}

fn body(path: &std::path::Path) -> String {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    text.split_once("---")
        .and_then(|(_, rest)| rest.split_once("---"))
        .map(|(_, rest)| rest.trim().to_string())
        .unwrap_or_else(|| panic!("{} has no frontmatter", path.display()))
}

fn procedure(path: &std::path::Path) -> String {
    let text =
        std::fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    text.split_once("The change stays")
        .map(|(_, rest)| format!("The change stays{rest}"))
        .unwrap_or_else(|| panic!("{} has no procedure", path.display()))
}
