use crate::provider::ToolDefinition;
use crate::tools::tool;

pub(crate) fn definitions() -> Vec<ToolDefinition> {
    let none =
        serde_json::json!({"type": "object", "properties": {}, "additionalProperties": false});
    [
        (
            "agents",
            "Live agent sessions on this machine: id, kind, model, workspace, status.",
            none.clone(),
        ),
        (
            "send",
            "Send text to one agent by id, or to \"all\". It lands in their inbox; they reply to your id.",
            serde_json::json!({"type": "object", "properties": {"to": {"type": "string"}, "text": {"type": "string"}}, "required": ["to", "text"], "additionalProperties": false}),
        ),
        ("inbox", "Read mail that arrived since the last read.", none),
        (
            "board",
            "The workspace board shared by agents here: list, get key, set key=value. Claim files, post decisions, split work.",
            serde_json::json!({"type": "object", "properties": {"action": {"type": "string", "enum": ["get", "set", "list"]}, "key": {"type": "string"}, "value": {"type": "string"}}, "required": ["action"], "additionalProperties": false}),
        ),
    ]
    .into_iter()
    .map(|(name, description, schema)| tool(name, description, schema))
    .collect()
}

pub(crate) fn is_mail_tool(name: &str) -> bool {
    matches!(name, "agents" | "inbox" | "board" | "send")
}
