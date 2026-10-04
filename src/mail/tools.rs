use crate::provider::ToolDefinition;
use crate::tools::tool;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tool {
    Agents,
    Send,
    Inbox,
    Board,
}

const NAMES: &[(&str, Tool)] = &[
    ("agents", Tool::Agents),
    ("send", Tool::Send),
    ("inbox", Tool::Inbox),
    ("board", Tool::Board),
];

impl Tool {
    #[must_use]
    pub(crate) fn of(name: &str) -> Option<Self> {
        NAMES
            .iter()
            .find(|(known, _)| *known == name)
            .map(|(_, tool)| *tool)
    }

    #[must_use]
    pub(crate) fn name(self) -> &'static str {
        NAMES
            .iter()
            .find(|(_, tool)| *tool == self)
            .map_or("", |(name, _)| *name)
    }
}

pub(crate) fn definitions() -> Vec<ToolDefinition> {
    let none =
        serde_json::json!({"type": "object", "properties": {}, "additionalProperties": false});
    [
        (
            Tool::Agents,
            "Live agent sessions on this machine: id, kind, model, workspace, status.",
            none.clone(),
        ),
        (
            Tool::Send,
            "Send text to one agent by id, or to \"all\". It lands in their inbox; they reply to your id.",
            serde_json::json!({"type": "object", "properties": {"to": {"type": "string"}, "text": {"type": "string"}}, "required": ["to", "text"], "additionalProperties": false}),
        ),
        (Tool::Inbox, "Read mail that arrived since the last read.", none),
        (
            Tool::Board,
            "The workspace board shared by agents here: list, get key, set key=value. Claim files, post decisions, split work.",
            serde_json::json!({"type": "object", "properties": {"action": {"type": "string", "enum": ["get", "set", "list"]}, "key": {"type": "string"}, "value": {"type": "string"}}, "required": ["action"], "additionalProperties": false}),
        ),
    ]
    .into_iter()
    .map(|(kind, description, schema)| tool(kind.name(), description, schema))
    .collect()
}

pub(crate) fn is_mail_tool(name: &str) -> bool {
    Tool::of(name).is_some()
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
