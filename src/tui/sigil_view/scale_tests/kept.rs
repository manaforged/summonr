use super::children::{IDS, spawn, task_call};
use super::{State, busy_info, frame_with};
use crate::tools::Event;
use crate::tui::Msg;

#[test]
fn a_child_kept_after_a_conflict_says_so_and_how_to_remove_it() {
    let record = serde_json::json!({
        "version": 1,
        "id": IDS[0],
        "parent": "01a104ac00000000000000000000ffff",
        "assignment": "append a License section",
        "model": "m",
        "workspace": "/work/csvtidy-children/18e0e8cdbf5d9778",
        "depth": 1,
        "access": "implement",
        "background": false,
        "call": "t1",
        "state": {"status": "completed", "result": {
            "text": "Done.",
            "changed_paths": [],
            "changed_paths_total": 0,
            "rounds": 4,
            "usage": {"prompt_tokens": 10, "completion_tokens": 5, "reasoning_tokens": 0, "cached_tokens": 0, "cache_write_tokens": 0},
            "billed_tokens": 15,
            "elapsed_millis": 25950,
            "diagnostic": "conflicts with the parent and was kept",
            "kept": {"checkout": "/work/csvtidy-children/18e0e8cdbf5d9778", "hold": "conflict"},
        }},
    });
    let shown = frame_with(busy_info(), 140, 40, State::Idle, |app| {
        task_call(app);
        spawn(app, IDS[0], "append a License section", false);
        app.update(Msg::Event(Event::ToolResult {
            id: "t1".to_owned(),
            success: true,
            mutated: true,
            output: record.to_string(),
            millis: None,
            patch: None,
        }));
    })
    .join("\n");
    assert!(
        shown.contains("conflicts with the parent") && shown.contains("rm -rf"),
        "{shown}"
    );
}
