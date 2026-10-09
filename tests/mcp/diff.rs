use serde_json::{Value, json};

use crate::support::{Model, Session, Step};

fn write(path: &'static str) -> Step {
    Step::Call("write", json!({"path": path, "content": "x\n"}))
}

pub(crate) fn turn_last_shows_the_latest_turn_of_a_resumed_session_without_generated_files() {
    let session = Session::new();
    session.run(
        &Model::start(vec![write("a.txt"), Step::Text("ok")]),
        "",
        &[],
    );
    let second = Model::start(vec![
        write("b.txt"),
        write("__pycache__/m.cpython-312.pyc"),
        write(".summonr/review.json"),
        Step::Text("ok"),
    ]);
    session.run(&second, "", &["-c"]);
    let (ok, text) = session.cli(&["diff", "--turn", "last", "--json"]);
    assert!(ok, "{text}");
    let diff: Value = serde_json::from_str(text.trim()).unwrap_or_else(|_| panic!("{text}"));
    let files: Vec<&str> = diff["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|file| file["path"].as_str())
        .collect();
    assert_eq!(files, ["b.txt"], "{text}");
}

pub(crate) fn a_turn_diff_names_its_session() {
    let session = Session::new();
    session.run(
        &Model::start(vec![write("a.txt"), Step::Text("ok")]),
        "",
        &[],
    );
    let (ok, text) = session.cli(&["diff", "--turn", "last", "--stat"]);
    assert!(ok && text.contains("turn 1 of session "), "{text}");
    let (_, json) = session.cli(&["diff", "--turn", "last", "--json"]);
    let diff: Value = serde_json::from_str(json.trim()).unwrap_or_else(|_| panic!("{json}"));
    assert!(
        diff["session"].as_str().is_some_and(|id| !id.is_empty()),
        "{json}"
    );
}
