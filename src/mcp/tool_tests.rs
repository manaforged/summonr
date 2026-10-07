#[test]
fn a_read_only_hint_does_not_make_an_mcp_tool_read_only() {
    let item = serde_json::json!({
        "name": "list",
        "annotations": {"readOnlyHint": true}
    });
    let tool = super::Tool::parse("srv", &item).unwrap();
    assert!(!tool.readonly);
}
