#[test]
fn a_tool_alias_joins_server_and_name_with_safe_characters() {
    let item = serde_json::json!({"name": "list.files"});
    let tool = super::Tool::parse("srv", &item).unwrap();
    assert_eq!(tool.alias, "srv__list_files");
}
