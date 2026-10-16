use super::*;

#[test]
fn a_plugin_lsp_file_becomes_servers_with_its_root_env_and_languages() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(FILE),
        serde_json::json!({"svelte": {
            "command": "${CLAUDE_PLUGIN_ROOT}/bin/svelteserver",
            "args": ["--stdio"],
            "env": {"HOME_DIR": "${CLAUDE_PLUGIN_ROOT}"},
            "extensionToLanguage": {".svelte": "svelte"}
        }})
        .to_string(),
    )
    .unwrap();
    let plugin = Plugin::at("ui".to_string(), dir.path().to_owned());
    let [server] = of(&plugin, workspace.path()).try_into().unwrap();
    let root = dir.path().display().to_string();
    assert_eq!(server.key, "ui:svelte");
    assert_eq!(
        server.argv,
        [format!("{root}/bin/svelteserver"), "--stdio".to_string()]
    );
    assert_eq!(server.env.get("HOME_DIR"), Some(&root));
    assert_eq!(
        server.languages.get("svelte").map(String::as_str),
        Some("svelte")
    );
    assert!(server.handles(Path::new("App.svelte")));
    assert!(!server.handles(Path::new("main.rs")));
}

#[test]
fn an_lsp_entry_without_a_command_does_not_hide_the_others() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(FILE),
        serde_json::json!({
            "pyls": {"command": "pylsp", "extensionToLanguage": {".py": "python"}},
            "broken": {"extensionToLanguage": {".x": "x"}}
        })
        .to_string(),
    )
    .unwrap();
    let plugin = Plugin::at("py".to_string(), dir.path().to_owned());
    let keys: Vec<String> = of(&plugin, dir.path())
        .into_iter()
        .map(|server| server.key)
        .collect();
    assert_eq!(keys, ["py:pyls"]);
}
