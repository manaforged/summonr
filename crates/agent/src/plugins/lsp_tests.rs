use super::*;

#[test]
fn a_plugin_lsp_file_becomes_servers_with_its_root_env_and_languages() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(LSP.file),
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
        dir.path().join(LSP.file),
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

#[test]
fn an_lsp_entry_expands_environment_placeholders() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(LSP.file),
        serde_json::json!({"go": {
            "command": "gopls",
            "args": ["-logfile=${SUMMONR_LSP_TEST_UNSET:-/opt/gopls.log}"],
            "env": {"GOPATH": "${SUMMONR_LSP_TEST_UNSET:-/opt/go}"},
            "extensionToLanguage": {".go": "go"}
        }})
        .to_string(),
    )
    .unwrap();
    let plugin = Plugin::at("go".to_string(), dir.path().to_owned());
    let [server] = of(&plugin, dir.path()).try_into().unwrap();
    assert_eq!(server.argv, ["gopls", "-logfile=/opt/gopls.log"]);
    assert_eq!(
        server.env.get("GOPATH").map(String::as_str),
        Some("/opt/go")
    );
}

#[test]
fn inline_lsp_servers_in_the_manifest_load_with_their_options_and_folder() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        dir.path().join(super::super::MANIFEST),
        serde_json::json!({"name": "go-lsp", "lspServers": {"go": {
            "command": "gopls",
            "args": ["serve"],
            "extensionToLanguage": {".go": "go"},
            "initializationOptions": {"staticcheck": true},
            "workspaceFolder": "mod",
            "startupTimeout": 5000
        }}})
        .to_string(),
    )
    .unwrap();
    let plugin = Plugin::at("go-lsp".to_string(), dir.path().to_owned());
    let [server] = of(&plugin, workspace.path()).try_into().unwrap();
    assert_eq!(server.key, "go-lsp:go");
    assert_eq!(server.argv, ["gopls", "serve"]);
    assert_eq!(server.options, serde_json::json!({"staticcheck": true}));
    assert_eq!(server.root, Some(workspace.path().join("mod")));
    assert_eq!(
        unused(&plugin),
        ["skipped\tlsp go startupTimeout (summonr does not apply it)"]
    );
}

#[test]
fn a_manifest_lsp_server_replaces_the_default_file_entry_of_the_same_name() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".claude-plugin")).unwrap();
    std::fs::write(
        dir.path().join(LSP.file),
        serde_json::json!({"py": {"command": "old", "extensionToLanguage": {".py": "python"}}})
            .to_string(),
    )
    .unwrap();
    std::fs::write(
        dir.path().join(super::super::MANIFEST),
        serde_json::json!({"name": "py", "lspServers": [
            "./extra.json",
            {"py": {"command": "new", "extensionToLanguage": {".py": "python"}}}
        ]})
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("extra.json"),
        serde_json::json!({"rb": {"command": "solargraph", "extensionToLanguage": {".rb": "ruby"}}})
            .to_string(),
    )
    .unwrap();
    let plugin = Plugin::at("py".to_string(), dir.path().to_owned());
    let argv: Vec<String> = of(&plugin, dir.path())
        .into_iter()
        .map(|server| format!("{} {}", server.key, server.argv.join(" ")))
        .collect();
    assert_eq!(argv, ["py:py new", "py:rb solargraph"]);
}
