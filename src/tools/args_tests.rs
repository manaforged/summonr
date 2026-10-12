use super::tool;
use crate::policy::Policy;
use crate::tools::ToolRegistry;

#[test]
fn every_tool_schema_root_is_a_plain_object() {
    let dir = tempfile::tempdir().unwrap();
    let tools = ToolRegistry::with_policy_and_search(dir.path(), Policy::builtin(), None).unwrap();
    let server = tool(
        "srv__pick",
        "",
        serde_json::json!({"oneOf": [
            {"type": "object", "properties": {"id": {"type": "string"}}, "required": ["id"]},
            {"type": "string"}
        ]}),
    );
    for definition in tools.definitions().into_iter().chain([server.clone()]) {
        let root = &definition.function.parameters;
        assert_eq!(root["type"], "object", "{}", definition.function.name);
        for union in ["anyOf", "oneOf", "allOf"] {
            assert!(
                root.get(union).is_none(),
                "{} {union}",
                definition.function.name
            );
        }
    }
    assert_eq!(
        server.function.parameters["properties"]["id"]["type"],
        "string"
    );
}
