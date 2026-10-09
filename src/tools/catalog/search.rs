use crate::provider::ToolDefinition;
use crate::tools::tool;
use crate::tools::workspace::search::{CONTEXT_MAX, GREP_DEFAULT, GREP_MAX};

pub(super) fn search() -> ToolDefinition {
    tool(
        "search",
        &format!(
            "Find text in files, or files by name, anywhere on disk; use it instead of grep, rg, find, or mdfind. pattern searches contents, name and glob match file names, and together they search only the matching files. Case is ignored, results are newest first, a call returns what it found after {secs}s, and secret files are never read.",
            secs = crate::limits::limits().search_secs
        ),
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Regex over file contents; a|b for alternatives."},
                "name": {"type": "string", "description": "Words that must all appear in the file name, any order: \"q3 report\"."},
                "glob": {"type": "string", "description": "File pattern such as *.rs or src/**/*.ts."},
                "path": {"type": "string", "description": "Folder or file to search; ~ and absolute paths work. Default: the workspace."},
                "output": {"type": "string", "enum": ["lines", "files", "count", "folders"], "description": "lines (default) shows matches; files lists matching files; count counts per file; folders counts files per folder."},
                "context": {"type": "integer", "minimum": 0, "maximum": CONTEXT_MAX, "description": "Lines around each match."},
                "newer": {"type": "string", "description": "Only files changed within 30m, 2h, 3d, 2w."},
                "word": {"type": "boolean", "description": "Whole-word match."},
                "literal": {"type": "boolean", "description": "Pattern is plain text, not a regex."},
                "case_sensitive": {"type": "boolean", "description": "Match case exactly."},
                "no_ignore": {"type": "boolean", "description": "Include gitignored files and build folders."},
                "depth": {"type": "integer", "minimum": 1, "description": "Folder levels to descend."},
                "limit": {"type": "integer", "minimum": 1, "maximum": GREP_MAX, "description": format!("Results to return (default {GREP_DEFAULT}).")},
                "offset": {"type": "integer", "minimum": 0, "description": "Files to skip, for the next page."},
                "tags": {"type": "boolean", "description": "Print N:hash tags that edit takes."}
            },
            "additionalProperties": false
        }),
    )
}
