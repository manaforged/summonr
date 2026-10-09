use crate::provider::ToolDefinition;
use crate::tools::tool;
use crate::tools::workspace::search::{CONTEXT_MAX, GREP_DEFAULT, GREP_MAX};

pub(super) fn search() -> ToolDefinition {
    tool(
        "search",
        &format!(
            "Find text in files, or files by name, anywhere on disk; use it instead of grep, rg, find, or mdfind. pattern searches contents; name finds files whose names contain every word (\"q3 report\"); glob matches a file pattern (*.rs). Combine them to search contents of matching files only. path or paths pick where: the workspace by default, ~ and / work, and a call returns what it found after {secs}s. Case is ignored unless case_sensitive. Results are newest first. Secret files are never read.",
            secs = crate::limits::limits().search_secs
        ),
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string", "description": "Regex matched against file contents. Several alternatives: a|b. Omit it to list files by name or glob."},
                "name": {"type": "string", "description": "Words that must all appear in the file name, in any order, any case: \"q3 report\" finds Q3-Report-FINAL.txt."},
                "glob": {"type": "string", "description": "File pattern such as *.rs or src/**/*.ts. Without wildcards it matches names containing it."},
                "path": {"type": "string", "description": "Folder or file to search: relative to the workspace, absolute, or ~."},
                "paths": {"type": "array", "items": {"type": "string"}, "description": "Several folders or files to search in one call."},
                "exclude": {"type": "string", "description": "Glob of files to leave out, such as *.min.js."},
                "type": {"type": "string", "description": "File type name such as rust, py, js, ts, go, md."},
                "word": {"type": "boolean", "description": "Match pattern as a whole word only."},
                "literal": {"type": "boolean", "description": "Treat pattern as plain text, not a regex."},
                "case_sensitive": {"type": "boolean", "description": "Match case exactly."},
                "files": {"type": "boolean", "description": "With pattern: list the files whose contents match, not the lines."},
                "count": {"type": "boolean", "description": "With pattern: count matching lines per file."},
                "group": {"type": "string", "enum": ["folder"], "description": "Count matching files per folder, largest first, instead of listing them."},
                "newer": {"type": "string", "description": "Only files modified within this long: 30m, 2h, 3d, 2w."},
                "context": {"type": "integer", "minimum": 0, "maximum": CONTEXT_MAX, "description": "Lines to show around each hit."},
                "before": {"type": "integer", "minimum": 0, "maximum": CONTEXT_MAX, "description": "Lines to show before each hit."},
                "after": {"type": "integer", "minimum": 0, "maximum": CONTEXT_MAX, "description": "Lines to show after each hit."},
                "depth": {"type": "integer", "minimum": 1, "description": "How many folder levels deep to look."},
                "multiline": {"type": "boolean", "description": "Let pattern match across lines."},
                "no_ignore": {"type": "boolean", "description": "Also search gitignored files and build folders such as node_modules and target."},
                "tags": {"type": "boolean", "description": "Print path:N:hash|line; N:hash is the tag edit takes."},
                "limit": {"type": "integer", "minimum": 1, "maximum": GREP_MAX, "description": format!("Most results to return (default {GREP_DEFAULT}).")},
                "offset": {"type": "integer", "minimum": 0, "description": "Skip this many files to see the next page."}
            },
            "additionalProperties": false
        }),
    )
}
