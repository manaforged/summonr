use std::path::Path;

use serde_json::{Value, json};

use crate::support::{Model, Options, Session, Step, read};

const IMPORT: &str = "import_claude_settings = true\n";

fn settings(root: &Path, doc: &Value) {
    let dir = root.join(".claude");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("settings.json"), doc.to_string()).unwrap();
}

fn imported(root: &'static str) -> Options<'static> {
    Options {
        root,
        ..Options::default()
    }
}

#[cfg(unix)]
pub(crate) fn claude_settings_hooks_and_env_load_only_when_imported() {
    let session = Session::new();
    let hook = "input=$(cat); echo \"$input\" | grep -q 'rm -rf' && { echo \"blocked by settings hook $TEAM\" >&2; exit 2; }; exit 0";
    settings(
        session.work.path(),
        &json!({
            "env": {"TEAM": "red"},
            "hooks": {"PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": hook}]}]}
        }),
    );
    for (root, on) in [(IMPORT, true), ("", false)] {
        let model = Model::start(vec![
            Step::Call(
                "run_command",
                json!({"command": "printf 'team=%s' \"$TEAM\""}),
            ),
            Step::Call("run_command", json!({"command": "rm -rf scratch"})),
            Step::Text("ok"),
        ]);
        session.run_with(&model, "", &[], &imported(root));
        let results = model.tool_results();
        assert_eq!(results[0].contains("team=red"), on, "{results:?}");
        assert_eq!(
            results[1].contains("blocked by settings hook red"),
            on,
            "{results:?}"
        );
    }
}

pub(crate) fn a_project_deny_rule_holds_only_in_a_trusted_workspace() {
    let session = Session::new();
    settings(
        session.work.path(),
        &json!({"permissions": {"deny": ["mcp__fixture1__echo"]}}),
    );
    for (flags, denied) in [
        (&["--strict"][..], true),
        (&["--strict", "--no-trust"], false),
    ] {
        let model = Model::start(vec![
            Step::Call("fixture1__echo", json!({"text": "x"})),
            Step::Text("ok"),
        ]);
        session.run_with(
            &model,
            &session.server("fixture1", ""),
            flags,
            &imported(IMPORT),
        );
        let results = model.tool_results();
        assert_eq!(
            results[0].contains("denied by policy `fixture1__echo`"),
            denied,
            "{flags:?}: {results:?}"
        );
    }
}

pub(crate) fn a_malformed_settings_file_is_a_note_and_the_session_runs() {
    let session = Session::new();
    let dir = session.work.path().join(".claude");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("settings.json"), "{ \"env\": ").unwrap();
    let model = Model::start(vec![Step::Text("ok")]);
    let outcome = session.run_with(&model, "", &[], &imported(IMPORT));
    let stderr = read(&session.file("stderr"));
    assert_eq!(outcome.code, Some(0), "{stderr}");
    assert!(
        stderr.contains("summonr skipped this Claude Code settings file"),
        "{stderr}"
    );
}
