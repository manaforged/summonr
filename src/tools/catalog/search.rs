use crate::provider::ToolDefinition;
use crate::tools::tool;
use crate::tools::workspace::search::{CONTEXT_MAX, GREP_MAX};

pub(super) fn search() -> ToolDefinition {
    tool(
        "search",
        "Find text in files (pattern) or files by name (name, glob), anywhere on disk; use it instead of grep, rg, or find. Newest first; secret files are never read. op=help lists the other options.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "name": {"type": "string"},
                "glob": {"type": "string"},
                "path": {"type": "string"},
                "output": {"type": "string", "enum": ["lines", "files", "count", "folders"]},
                "context": {"type": "integer", "minimum": 0, "maximum": CONTEXT_MAX},
                "literal": {"type": "boolean"},
                "limit": {"type": "integer", "minimum": 1, "maximum": GREP_MAX},
                "op": {"type": "string", "enum": ["help"]}
            }
        }),
    )
}
