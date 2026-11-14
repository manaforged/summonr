use serde_json::{Value, json};

use crate::support::{Model, Step, repository};
use crate::{Editor, asked, choose, reply};

const FEATURES: [&str; 7] = [
    "session/list",
    "session/diff",
    "session/model",
    "steer",
    "session/status",
    "session/child",
    "usage",
];

fn notes<'a>(editor: &'a Editor, method: &str) -> Vec<&'a Value> {
    editor
        .seen
        .iter()
        .filter(|frame| frame["method"] == method)
        .map(|frame| &frame["params"])
        .collect()
}

fn finish(editor: &mut Editor) -> Value {
    loop {
        let frame = editor.until("prompt reply or permission", |frame| {
            asked(frame) || frame["id"] == 3
        });
        if !asked(&frame) {
            return frame;
        }
        choose(editor, &frame, "allow_once");
    }
}

#[test]
fn without_the_opt_in_every_extension_is_method_not_found() {
    let home = tempfile::tempdir().expect("temp home");
    let model = Model::start(Vec::new());
    let mut editor = Editor::open(home.path(), &model);
    let init = editor.initialize();
    let session = editor.start(home.path());
    assert!(
        init["result"]["agentCapabilities"]["_meta"].is_null(),
        "{init}"
    );
    let list = editor.call(4, "_summonr.dev/session/list", json!({}));
    assert_eq!(list["error"]["code"], -32601, "{list}");
    let steer = json!({"sessionId": session, "prompt": [{"type": "text", "text": "hi"}]});
    let steer = editor.call(5, "_summonr.dev/steer", steer);
    assert_eq!(steer["error"]["code"], -32601, "{steer}");
}

#[test]
fn the_opt_in_echoes_the_version_and_every_served_feature() {
    let home = tempfile::tempdir().expect("temp home");
    let model = Model::start(Vec::new());
    let mut editor = Editor::open(home.path(), &model);
    let init = editor.opt_in();
    let ext = &init["result"]["agentCapabilities"]["_meta"]["summonr.dev/ext"];
    assert_eq!(ext["version"], 1, "{init}");
    assert_eq!(ext["features"], json!(FEATURES), "{init}");
    let unknown = editor.call(2, "_summonr.dev/nope", json!({}));
    assert_eq!(unknown["error"]["code"], -32601, "{unknown}");
}

#[test]
fn after_a_writing_turn_list_status_usage_and_diff_report_it() {
    let home = tempfile::tempdir().expect("temp home");
    let work = repository();
    let model = Model::start(vec![
        Step::Call("write", json!({"path": "a.txt", "content": "x\n"})),
        Step::Text("done"),
    ]);
    let mut editor = Editor::open(home.path(), &model);
    let session = editor.ext_session(work.path());
    editor.prompt(&session, "write a.txt");
    assert_eq!(finish(&mut editor)["result"]["stopReason"], "end_turn");

    let states: Vec<&Value> = notes(&editor, "_summonr.dev/session/status")
        .into_iter()
        .map(|note| &note["status"])
        .collect();
    assert_eq!(
        states.first().copied(),
        Some(&json!("working")),
        "{states:?}"
    );
    assert_eq!(states.last().copied(), Some(&json!("done")), "{states:?}");
    let usage = notes(&editor, "_summonr.dev/usage");
    let usage = usage.first().copied().expect("a usage notification");
    assert_eq!(usage["sessionId"], session.as_str(), "{usage}");
    assert_eq!(usage["endpoint"], "local", "{usage}");
    assert_eq!(usage["limit"], 32768, "{usage}");
    assert!(
        usage["used"].as_u64().is_some_and(|used| used > 0),
        "{usage}"
    );

    let list = editor.call(4, "_summonr.dev/session/list", json!({}));
    let rows = list["result"]["sessions"].as_array().expect("sessions");
    let row = rows
        .iter()
        .find(|row| row["id"] == session.as_str())
        .unwrap_or_else(|| panic!("no {session} in {list}"));
    assert!(row["format"].is_u64(), "{row}");

    let diff = json!({"sessionId": session, "turn": "last"});
    let diff = editor.call(5, "_summonr.dev/session/diff", diff);
    let paths: Vec<&str> = diff["result"]["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|file| file["path"].as_str())
        .collect();
    assert_eq!(paths, ["a.txt"], "{diff}");
}

#[test]
fn a_model_switch_reaches_the_next_model_request() {
    let home = tempfile::tempdir().expect("temp home");
    let model = Model::start(vec![Step::Text("done")]);
    let mut editor = Editor::open(home.path(), &model);
    let session = editor.ext_session(home.path());
    let switch = json!({"sessionId": session, "endpoint": "local", "model": "m2"});
    let switched = editor.call(4, "_summonr.dev/session/model", switch);
    assert_eq!(switched["result"], json!({}), "{switched}");
    editor.prompt(&session, "hi");
    assert_eq!(reply(&mut editor)["result"]["stopReason"], "end_turn");
    let requests = model.requests();
    let last = requests.last().expect("a model request");
    assert_eq!(last["model"], "m2", "{last}");
}

#[test]
fn the_schema_names_every_feature_and_the_served_protocol_version() {
    let home = tempfile::tempdir().expect("temp home");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_summonr"))
        .args(["acp", "--schema"])
        .current_dir(home.path())
        .env_clear()
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("SUMMONR_HOME", home.path())
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .output()
        .expect("run summonr acp --schema");
    assert!(output.status.success(), "{output:?}");
    let schema: Value = serde_json::from_slice(&output.stdout).expect("the schema is JSON");
    let model = Model::start(Vec::new());
    let mut editor = Editor::open(home.path(), &model);
    let init = editor.opt_in();
    let echoed = &init["result"]["agentCapabilities"]["_meta"]["summonr.dev/ext"]["features"];
    let echoed: Vec<&str> = echoed
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(echoed, FEATURES, "{init}");
    assert_eq!(
        schema["extension"]["features"],
        json!(FEATURES),
        "{schema:#}"
    );
    for feature in FEATURES {
        let method = format!("_summonr.dev/{feature}");
        let named = [
            &schema["methods"][&method],
            &schema["notifications"][&method],
        ];
        assert!(
            named.iter().any(|entry| entry.is_object()),
            "no {method} in {schema:#}"
        );
    }
    assert_eq!(
        schema["protocolVersion"], init["result"]["protocolVersion"],
        "{init}"
    );
    assert_eq!(schema["version"], 1, "{schema:#}");
    let definitions = schema["definitions"]
        .as_object()
        .map_or(0, serde_json::Map::len);
    assert!(definitions > 0, "{schema:#}");
}
