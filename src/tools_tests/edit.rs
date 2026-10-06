use super::*;

#[test]
fn writing_over_a_file_reports_a_summary_not_the_whole_diff() {
    let (_dir, tools) = workspace(&[("a.txt", "old one\nold two\nold three\n")]);
    let text = run(
        &tools,
        "write",
        serde_json::json!({"path": "a.txt", "content": "new\n"}),
    );
    assert!(text.contains("replaced"), "{text}");
    assert!(!text.contains("-old one"), "{text}");
}

#[test]
fn two_edits_on_one_line_both_apply() {
    let (dir, tools) = workspace(&[("a.py", "a = x.astype(n.int) + y.astype(n.bool)\n")]);
    let text = run(
        &tools,
        "edit",
        serde_json::json!({"path": "a.py", "edits": [
            {"old": ".astype(n.int)", "new": ".astype(int)"},
            {"old": ".astype(n.bool)", "new": ".astype(bool)"}
        ]}),
    );
    assert!(!text.contains("overlap"), "{text}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.py")).unwrap(),
        "a = x.astype(int) + y.astype(bool)\n"
    );
}

#[test]
fn an_edit_with_unknown_keys_names_them() {
    let (_dir, tools) = workspace(&[("a.txt", "one\n")]);
    let call = ToolCall {
        id: "t1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "edit".to_string(),
            arguments: serde_json::json!({"path": "a.txt", "old_tags": "1:ab", "new": "two"})
                .to_string(),
        },
        continuation: None,
    };
    let outcome = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(tools.execute_outcome(&call));
    let Err(Error::Tool(message)) = outcome else {
        panic!("expected a tool error");
    };
    assert!(message.contains("old_tags"), "{message}");
}

#[test]
fn a_large_edit_returns_a_short_diff() {
    let before: String = (0..200).map(|n| format!("old {n}\n")).collect();
    let after: String = (0..200).map(|n| format!("new {n}\n")).collect();
    let (_dir, tools) = workspace(&[("a.txt", &before)]);
    let text = run(
        &tools,
        "edit",
        serde_json::json!({"path": "a.txt", "old": before, "new": after}),
    );
    assert!(text.len() < 200, "{text}");
    assert!(text.contains("lines 1-200"), "{text}");
}
