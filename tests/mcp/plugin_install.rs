use std::path::Path;

use serde_json::json;

use crate::support::{Model, Session, Step};
#[cfg(unix)]
use crate::support::{fixture, read};

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn skill(dir: &Path, name: &str, body: &str) {
    write(
        &dir.join(".claude-plugin/plugin.json"),
        &format!(r#"{{"name": "{name}"}}"#),
    );
    write(
        &dir.join("skills/greet/SKILL.md"),
        &format!("---\nname: greet\ndescription: hi\n---\n{body}"),
    );
}

pub(crate) fn plugin_install_refuses_a_tree_that_adds_nothing() {
    let session = Session::new();
    let dir = session.files.path().join("mods");
    write(
        &dir.join("hooks/hooks.json"),
        &json!({"hooks": {"PreToolUse": [{"hooks": [{"type": "prompt", "prompt": "x"}]}]}})
            .to_string(),
    );
    let (ok, text) = session.cli(&["install", &dir.display().to_string()]);
    assert!(!ok, "{text}");
    assert!(
        text.contains("plugin `mods` adds nothing summonr loads"),
        "{text}"
    );
    assert!(
        text.contains("skipped\tPreToolUse prompt (summonr runs command hooks only)"),
        "{text}"
    );
    assert!(!session.home.path().join("plugins/mods").exists(), "{text}");
}

pub(crate) fn plugin_install_keeps_the_copy_when_json_does_not_parse() {
    let session = Session::new();
    let good = session.files.path().join("good-src");
    skill(&good, "demo", "Hi.\n");
    let (ok, text) = session.cli(&["install", &good.display().to_string()]);
    assert!(ok, "{text}");
    let installed = session
        .home
        .path()
        .join("plugins/demo/skills/greet/SKILL.md");
    let body = std::fs::read_to_string(&installed).unwrap();
    let bad = session.files.path().join("bad-src");
    skill(&bad, "demo", "replaced\n");
    write(&bad.join(".mcp.json"), "{");
    let (again, err) = session.cli(&["install", &bad.display().to_string()]);
    assert!(!again, "{err}");
    assert!(err.contains(".mcp.json"), "{err}");
    assert_eq!(std::fs::read_to_string(&installed).unwrap(), body, "{err}");
}

pub(crate) fn plugin_reinstall_keeps_plugin_data() {
    let session = Session::new();
    let src = session.files.path().join("demo-src");
    skill(&src, "demo", "first\n");
    let (ok, text) = session.cli(&["install", &src.display().to_string()]);
    assert!(ok, "{text}");
    let data = session.home.path().join("plugin-data/demo/keep.txt");
    write(&data, "kept");
    skill(&src, "demo", "second\n");
    let (again, replaced) = session.cli(&["install", &src.display().to_string()]);
    assert!(again, "{replaced}");
    assert_eq!(std::fs::read_to_string(&data).unwrap(), "kept");
    let skill_text = std::fs::read_to_string(
        session
            .home
            .path()
            .join("plugins/demo/skills/greet/SKILL.md"),
    )
    .unwrap();
    assert!(skill_text.contains("second"), "{skill_text}");
    assert!(!skill_text.contains("first"), "{skill_text}");
}

pub(crate) fn plugin_unknown_off_warns_without_blocking_list_or_session_start() {
    let session = Session::new();
    std::fs::write(
        session.home.path().join("config.toml"),
        "[plugins]\noff = [\"missing\"]\n",
    )
    .unwrap();
    let (ok, text) = session.cli(&["install", "--list"]);
    assert!(ok, "{text}");
    assert!(text.contains("not installed: missing"), "{text}");
    let model = Model::start(vec![Step::Text("ok")]);
    let outcome = session.run(&model, "[plugins]\noff = [\"missing\"]\n", &[]);
    assert_eq!(outcome.code, Some(0), "{}", outcome.stdout);
    let err = std::fs::read_to_string(session.file("stderr")).unwrap_or_default();
    assert!(err.contains("not installed: missing"), "{err}");
}

pub(crate) fn plugin_uninstall_removes_an_unknown_off_name() {
    let session = Session::new();
    std::fs::write(
        session.home.path().join("config.toml"),
        "[plugins]\noff = [\"typo\"]\n",
    )
    .unwrap();
    let (ok, text) = session.cli(&["uninstall", "typo"]);
    assert!(ok, "{text}");
    let (listed, list_text) = session.cli(&["install", "--list"]);
    assert!(listed, "{list_text}");
    assert!(list_text.contains("no plugins installed"), "{list_text}");
}

#[cfg(unix)]
fn wrapped(session: &Session, token: &str) -> std::path::PathBuf {
    let dir = session.files.path().join("wrap-source");
    write(
        &dir.join(".claude-plugin/plugin.json"),
        r#"{"name": "demo"}"#,
    );
    let servers = json!({"mcpServers": {"fx": {
        "command": "sh",
        "args": ["-c", "printf %s \"$TOKEN\" > \"$OUT\"; exec \"$REAL\""],
        "env": {
            "TOKEN": token,
            "OUT": session.file("token.txt").display().to_string(),
            "REAL": fixture().display().to_string(),
            "MCP_FIXTURE_SERVE": "1",
            "MCP_FIXTURE_LOG": session.file("wrap.log").display().to_string()
        }
    }}});
    write(&dir.join(".mcp.json"), &servers.to_string());
    dir
}

#[cfg(unix)]
pub(crate) fn plugin_user_config_reaches_the_server() {
    let session = Session::new();
    let (ok, text) = session.cli(&[
        "install",
        &wrapped(&session, "${user_config.api_token}")
            .display()
            .to_string(),
    ]);
    assert!(ok, "{text}");
    let model = Model::start(vec![
        Step::Call("demo-fx__echo", json!({"text": "x"})),
        Step::Text("ok"),
    ]);
    session.run(
        &model,
        "[mcp.demo-fx.user_config]\napi_token = \"s3cret\"\n",
        &[],
    );
    assert_eq!(read(&session.file("token.txt")), "s3cret");
}

#[cfg(unix)]
pub(crate) fn plugin_user_env_replaces_the_server_env() {
    let session = Session::new();
    let (ok, text) = session.cli(&[
        "install",
        &wrapped(&session, "plugin-value").display().to_string(),
    ]);
    assert!(ok, "{text}");
    let model = Model::start(vec![
        Step::Call("demo-fx__echo", json!({"text": "x"})),
        Step::Text("ok"),
    ]);
    session.run(&model, "[mcp.demo-fx.env]\nTOKEN = \"from-env\"\n", &[]);
    assert_eq!(read(&session.file("token.txt")), "from-env");
}

#[cfg(unix)]
pub(crate) fn plugin_non_command_hook_is_skipped_and_does_not_run() {
    let session = Session::new();
    let dir = session.files.path().join("demo-source");
    write(
        &dir.join(".claude-plugin/plugin.json"),
        r#"{"name": "demo"}"#,
    );
    let servers = json!({"mcpServers": {"fx": {
        "command": "${CLAUDE_PLUGIN_ROOT}/bin/serve",
        "env": {"MCP_FIXTURE_SERVE": "1", "MCP_FIXTURE_LOG": session.file("fixture1.log").display().to_string()}
    }}});
    write(&dir.join(".mcp.json"), &servers.to_string());
    let bin = dir.join("bin/serve");
    std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(fixture(), &bin).unwrap();
    write(
        &dir.join("hooks/hooks.json"),
        &json!({"hooks": {"PreToolUse": [{"hooks": [{"type": "prompt", "prompt": "no"}]}]}})
            .to_string(),
    );
    let (ok, text) = session.cli(&["install", &dir.display().to_string()]);
    assert!(ok, "{text}");
    assert!(
        text.contains("skipped\tPreToolUse prompt (summonr runs command hooks only)"),
        "{text}"
    );
    let (listed, list_text) = session.cli(&["install", "--list"]);
    assert!(listed, "{list_text}");
    assert!(
        list_text.contains("skipped\tPreToolUse prompt (summonr runs command hooks only)"),
        "{list_text}"
    );
    let model = Model::start(vec![
        Step::Call("demo-fx__echo", json!({"text": "ran"})),
        Step::Text("ok"),
    ]);
    session.run(&model, "", &[]);
    assert!(
        model.tool_results()[0].contains("ran"),
        "{:?}",
        model.tool_results()
    );
}

pub(crate) fn plugin_install_skips_a_server_turned_off_in_config() {
    let session = Session::new();
    std::fs::write(
        session.home.path().join("config.toml"),
        "[mcp.demo-fx]\nenabled = false\n",
    )
    .unwrap();
    let dir = session.files.path().join("demo");
    write(
        &dir.join(".claude-plugin/plugin.json"),
        r#"{"name": "demo"}"#,
    );
    write(
        &dir.join(".mcp.json"),
        &json!({"mcpServers": {"fx": {"command": "no-such-mcp-server"}}}).to_string(),
    );
    let (ok, text) = session.cli(&["install", &dir.display().to_string()]);
    assert!(ok, "{text}");
    assert!(text.contains("demo-fx: off in config"), "{text}");
}

pub(crate) fn plugin_install_counts_only_language_servers_that_load() {
    let session = Session::new();
    let dir = session.files.path().join("lsp-only");
    write(
        &dir.join(".claude-plugin/plugin.json"),
        r#"{"name": "lsp-only"}"#,
    );
    write(
        &dir.join(".lsp.json"),
        &json!({"broken": {"extensionToLanguage": {".x": "x"}}}).to_string(),
    );
    let (ok, text) = session.cli(&["install", &dir.display().to_string()]);
    assert!(!ok, "{text}");
    assert!(
        text.contains("plugin `lsp-only` adds nothing summonr loads"),
        "{text}"
    );
}
