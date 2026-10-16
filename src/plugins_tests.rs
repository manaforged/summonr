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
