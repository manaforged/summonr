use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
use serde_json::Value;
use serde_json::json;

use crate::support::{Model, Session, Step};
#[cfg(unix)]
use crate::support::{advertised, fixture};

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

#[cfg(unix)]
fn executable(path: &Path, target: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(target, path).unwrap();
}

#[cfg(unix)]
fn plugin(session: &Session, hooks: &Value) -> PathBuf {
    let log = session.file("fixture1.log");
    plugin_with(
        session,
        hooks,
        &json!({"MCP_FIXTURE_SERVE": "1", "MCP_FIXTURE_LOG": log}),
        r#"{"name": "demo"}"#,
    )
}

#[cfg(unix)]
fn plugin_with(session: &Session, hooks: &Value, env: &Value, manifest: &str) -> PathBuf {
    let dir = session.files.path().join("demo-source");
    write(&dir.join(".claude-plugin/plugin.json"), manifest);
    let servers = json!({"mcpServers": {"fx": {
        "command": "${CLAUDE_PLUGIN_ROOT}/bin/serve",
        "env": env
    }}});
    write(&dir.join(".mcp.json"), &servers.to_string());
    executable(&dir.join("bin/serve"), &fixture());
    write(
        &dir.join("skills/greet/SKILL.md"),
        "---\nname: greet\ndescription: say hello politely\n---\nSay hello.\n",
    );
    write(
        &dir.join("commands/hello.md"),
        "---\ndescription: greet someone\n---\nGreet $1 warmly. Everyone: $ARGUMENTS",
    );
    write(
        &dir.join("agents/reviewer.md"),
        "---\nname: reviewer\ndescription: reviews a diff for bugs\ntools: Read, Grep\n---\nReview the diff.",
    );
    write(
        &dir.join("hooks/hooks.json"),
        &json!({ "hooks": hooks }).to_string(),
    );
    dir
}

#[cfg(unix)]
fn install(session: &Session, dir: &Path) -> String {
    let (ok, text) = session.cli(&["install", &dir.display().to_string()]);
    assert!(ok, "{text}");
    text
}

#[cfg(unix)]
pub(crate) fn a_claude_code_plugin_installs_and_every_part_loads() {
    let session = Session::new();
    let seen = session.file("hook-input.json");
    let hooks = json!({"PreToolUse": [{
        "matcher": "mcp__demo-fx__write",
        "hooks": [{"type": "command", "command": format!("cat > '{}'; echo blocked by plugin >&2; exit 2", seen.display())}]
    }]});
    let installed = install(&session, &plugin(&session, &hooks));
    assert!(!installed.contains("skipped\tcommands"), "{installed}");
    assert!(!installed.contains("skipped\tagents"), "{installed}");
    let model = Model::start(vec![
        Step::Call("fixture1__never", json!({})),
        Step::Call("run_command", json!({"command": "command -v serve"})),
        Step::Call("demo-fx__write", json!({})),
        Step::Call("demo-fx__echo", json!({"text": "through"})),
        Step::Text("ok"),
    ]);
    session.run(&model, "", &[]);
    let first = model.requests()[0].clone();
    let tools = advertised(&first);
    assert!(tools.contains(&"demo-fx__echo".to_string()), "{tools:?}");
    assert!(
        first.to_string().contains("say hello politely"),
        "no skill in the prompt"
    );
    let results = model.tool_results();
    assert!(results[1].contains("plugins/demo/bin/serve"), "{results:?}");
    assert!(results[2].contains("blocked by plugin"), "{results:?}");
    assert!(results[3].contains("through"), "{results:?}");
    let input: Value = serde_json::from_str(&std::fs::read_to_string(&seen).unwrap()).unwrap();
    assert_eq!(input["hook_event_name"], json!("PreToolUse"), "{input}");
    assert_eq!(input["tool_name"], json!("mcp__demo-fx__write"), "{input}");
}

#[cfg(unix)]
pub(crate) fn an_uninstalled_or_disabled_plugin_does_not_load() {
    let session = Session::new();
    install(&session, &plugin(&session, &json!({})));
    let disabled = Model::start(vec![Step::Text("ok")]);
    session.run(&disabled, "[plugins]\noff = [\"demo\"]\n", &[]);
    let tools = advertised(&disabled.requests()[0]);
    assert!(
        !tools.iter().any(|tool| tool.starts_with("demo-")),
        "{tools:?}"
    );
    let (removed, text) = session.cli(&["uninstall", "demo"]);
    assert!(removed, "{text}");
    let gone = Model::start(vec![Step::Text("ok")]);
    session.run(&gone, "", &[]);
    let tools = advertised(&gone.requests()[0]);
    assert!(
        !tools.iter().any(|tool| tool.starts_with("demo-")),
        "{tools:?}"
    );
    let (_, listed) = session.cli(&["install", "--list"]);
    assert!(listed.contains("no plugins installed"), "{listed}");
}

#[cfg(unix)]
pub(crate) fn a_claude_hook_blocks_only_on_exit_two_or_a_deny() {
    let session = Session::new();
    let deny = json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": "denied by json"}});
    let hooks = json!({"PreToolUse": [
        {"matcher": "mcp__demo-fx__echo", "hooks": [{"type": "command", "command": "echo not fatal >&2; exit 1"}]},
        {"matcher": "mcp__demo-fx__write", "hooks": [{"type": "command", "command": format!("echo '{deny}'")}]}
    ]});
    install(&session, &plugin(&session, &hooks));
    let model = Model::start(vec![
        Step::Call("demo-fx__echo", json!({"text": "kept"})),
        Step::Call("demo-fx__write", json!({})),
        Step::Text("ok"),
    ]);
    session.run(&model, "", &[]);
    let results = model.tool_results();
    assert!(results[0].contains("kept"), "{results:?}");
    assert!(results[1].contains("denied by json"), "{results:?}");
}

#[cfg(unix)]
const APPEND_ONE: &str = "echo one >> '{0}'";
#[cfg(unix)]
const APPEND_TWO: &str = "echo two >> '{0}'";
#[cfg(windows)]
const APPEND_ONE: &str = "[IO.File]::AppendAllText('{0}', 'one' + [char]10)";
#[cfg(windows)]
const APPEND_TWO: &str = "[IO.File]::AppendAllText('{0}', 'two' + [char]10)";

pub(crate) fn two_pre_tool_hooks_both_run_in_order() {
    let session = Session::new();
    let trail = session.file("trail.txt");
    let path = trail.display().to_string();
    let quoted = |hook: &str| serde_json::to_string(&hook.replace("{0}", &path)).unwrap();
    let config = format!(
        "[hooks]\npre_tool = [{}, {}]\n",
        quoted(APPEND_ONE),
        quoted(APPEND_TWO)
    );
    let model = Model::start(vec![
        Step::Call("run_command", json!({"command": "true"})),
        Step::Text("ok"),
    ]);
    session.run(&model, &config, &[]);
    assert_eq!(
        std::fs::read_to_string(&trail).unwrap_or_default(),
        "one\ntwo\n"
    );
}

pub(crate) fn a_legacy_bundle_installs_as_a_plugin() {
    let session = Session::new();
    let bundle = session.files.path().join("demo");
    write(
        &bundle.join("mcp.toml"),
        "[mcp.bundled]\ncommand = \"bundled-mcp\"\n",
    );
    let (installed, text) = session.cli(&["install", &bundle.display().to_string()]);
    assert!(installed, "{text}");
    let (_, listed) = session.cli(&["mcp", "list"]);
    assert!(listed.contains("demo-bundled"), "{listed}");
}

#[cfg(unix)]
pub(crate) fn a_chatty_hook_does_not_stall_the_call() {
    let session = Session::new();
    let config = "[hooks]\npre_tool = \"head -c 200000 /dev/zero | tr '\\\\0' x >&2; head -c 200000 /dev/zero | tr '\\\\0' y\"\n";
    let model = Model::start(vec![
        Step::Call("run_command", json!({"command": "echo ran"})),
        Step::Text("ok"),
    ]);
    let started = std::time::Instant::now();
    session.run(&model, config, &[]);
    assert!(
        started.elapsed().as_secs() < 30,
        "took {:?}",
        started.elapsed()
    );
    assert!(
        model.tool_results()[0].contains("ran"),
        "{:?}",
        model.tool_results()
    );
}

#[cfg(unix)]
pub(crate) fn plugin_settings_and_data_are_handled() {
    let session = Session::new();
    let env = json!({
        "MCP_FIXTURE_SERVE": "1",
        "MCP_FIXTURE_LOG": "${CLAUDE_PLUGIN_DATA}/fx.log",
        "TOKEN": "${user_config.api_token}"
    });
    let manifest = r#"{"name": "demo", "userConfig": {"api_token": {"type": "string", "title": "Token", "description": "Token", "sensitive": true}}}"#;
    let installed = install(&session, &plugin_with(&session, &json!({}), &env, manifest));
    assert!(installed.contains("skipped\tuserConfig"), "{installed}");
    assert!(
        installed.contains("warning\tmcp fx uses ${user_config.*}"),
        "{installed}"
    );
    let model = Model::start(vec![
        Step::Call("demo-fx__echo", json!({"text": "x"})),
        Step::Text("ok"),
    ]);
    session.run(&model, "", &[]);
    let data = session.home.path().join("plugin-data").join("demo");
    assert!(data.join("fx.log").is_file(), "{:?}", model.tool_results());
    let (removed, text) = session.cli(&["uninstall", "demo"]);
    assert!(removed, "{text}");
    assert!(!data.exists(), "plugin data was left behind");
}

#[cfg(unix)]
pub(crate) fn a_plugin_command_expands_into_the_prompt_and_its_agents_are_listed() {
    let session = Session::new();
    install(&session, &plugin(&session, &json!({})));
    for prompt in ["/demo:hello Ada Lovelace", "/hello Ada Lovelace"] {
        let model = Model::start(vec![Step::Text("ok")]);
        let options = crate::support::Options {
            prompt: Some(prompt),
            ..crate::support::Options::default()
        };
        session.run_with(&model, "", &[], &options);
        let first = model.requests()[0].to_string();
        assert!(
            first.contains("Greet Ada warmly. Everyone: Ada Lovelace"),
            "{prompt}: {first}"
        );
        assert!(
            first.contains("reviewer: reviews a diff for bugs"),
            "{first}"
        );
    }
}
