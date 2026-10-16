use super::*;

#[test]
fn a_tool_card_shows_workspace_paths_relative_in_its_output_too() {
    let mut root = String::new();
    let shown = frame_with(busy_info(), 120, 30, State::Busy, |app| {
        root = app.roots[0].clone();
        let file = format!("{root}/parser.py");
        app.update(Msg::Event(Event::ToolCall {
            id: "e1".to_owned(),
            name: "edit".to_owned(),
            arguments: format!(r#"{{"path":"{file}"}}"#),
        }));
        app.update(Msg::Event(Event::ToolResult {
            id: "e1".to_owned(),
            success: true,
            mutated: true,
            output: format!("edited {file}\nchanged lines 6-9 (+4 lines, 9 lines now)"),
            millis: Some(3),
            patch: None,
        }));
    })
    .join("\n");
    assert!(shown.contains("edited parser.py"), "{shown}");
    assert!(
        !shown.contains(&root),
        "no workspace path on screen: {shown}"
    );
}

#[test]
fn a_root_listing_and_a_patch_name_what_they_touch() {
    let shown = frame_with(busy_info(), 120, 30, State::Busy, |app| {
        let root = app.roots[0].clone();
        app.update(Msg::Event(Event::ToolCall {
            id: "l1".to_owned(),
            name: "read".to_owned(),
            arguments: serde_json::json!({ "op": "list", "path": root, "depth": 2 }).to_string(),
        }));
        let patch = format!(
            "*** Begin Patch\n*** Update File: {root}/pricing.py\n@@\n-a\n+b\n*** End Patch"
        );
        app.update(Msg::Event(Event::ToolCall {
            id: "p1".to_owned(),
            name: "apply_patch".to_owned(),
            arguments: serde_json::json!({ "patch": patch }).to_string(),
        }));
    })
    .join("\n");
    assert!(shown.contains("Read ./"), "{shown}");
    assert!(shown.contains("Edit pricing.py"), "{shown}");
}
