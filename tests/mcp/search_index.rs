use serde_json::json;

use crate::support::{Model, Session, Step};

#[cfg(unix)]
pub(crate) fn a_name_search_outside_the_workspace_answers_from_the_index_and_checks_the_disk() {
    let session = Session::new();
    let outside = session.files.path().join("archive");
    std::fs::create_dir_all(outside.join("2025")).unwrap();
    for name in ["2025/a-report.txt", "b-report.txt"] {
        std::fs::write(outside.join(name), "x").unwrap();
    }
    let root = outside.display().to_string();
    let config = "[limits]\nsearch_index_after_ms = 0\n";
    let model = Model::start(vec![
        Step::Call("search", json!({"name": "report", "path": root})),
        Step::Call("run_command", json!({"command": "sleep 2"})),
        Step::Call(
            "run_command",
            json!({"command": format!("rm {root}/b-report.txt && touch {root}/c-report.txt")}),
        ),
        Step::Call("search", json!({"name": "report", "path": root})),
        Step::Text("done"),
    ]);
    session.run(&model, config, &["--no-verify"]);
    let results = model.tool_results();
    assert!(results[0].contains("b-report.txt"), "{results:?}");
    assert!(!results[0].contains("file index"), "{results:?}");
    let indexed = &results[3];
    assert!(indexed.contains("from the file index"), "{results:?}");
    assert!(indexed.contains("2025/a-report.txt"), "{results:?}");
    assert!(!indexed.contains("b-report.txt"), "{results:?}");
    assert!(!indexed.contains("c-report.txt"), "{results:?}");
}
