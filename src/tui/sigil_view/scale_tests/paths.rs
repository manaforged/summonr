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
