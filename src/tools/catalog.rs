use crate::provider::ToolDefinition;

use super::{ToolRegistry, tool};

impl ToolRegistry {
    pub(crate) fn definitions(&self) -> Vec<ToolDefinition> {
        let mut all = vec![
            read_file(),
            replace_text(),
            replace_texts(),
            replace_lines(),
            insert_after(),
            delete_lines(),
            write_file(),
            list_dir(),
            grep(),
            skill(),
        ];
        if self.verification {
            all.extend([diagnostics(), definition(), run_command()]);
        }
        if self.search.is_some() {
            all.extend([web_search(), web_fetch()]);
        }
        all
    }

    pub(super) fn enabled(&self, name: &str) -> bool {
        match name {
            "read_file" | "list_dir" | "grep" | "skill" => true,
            "diagnostics" | "definition" => self.verification,
            "edit_file" | "replace_text" | "replace_texts" | "replace_lines" | "insert_after"
            | "delete_lines" | "write_file" => !self.policy.is_read_only(),
            "run_command" => self.verification && !self.policy.is_read_only(),
            "web_search" | "web_fetch" => self.search.is_some(),
            _ => false,
        }
    }
}

pub(crate) fn readonly(name: &str) -> bool {
    matches!(
        name,
        "read_file" | "list_dir" | "grep" | "diagnostics" | "skill" | "web_search" | "web_fetch"
    )
}

pub(super) fn known(name: &str) -> bool {
    matches!(
        name,
        "read_file"
            | "edit_file"
            | "replace_text"
            | "replace_texts"
            | "replace_lines"
            | "insert_after"
            | "delete_lines"
            | "write_file"
            | "list_dir"
            | "grep"
            | "diagnostics"
            | "definition"
            | "skill"
            | "run_command"
            | "web_search"
            | "web_fetch"
    )
}

fn read_file() -> ToolDefinition {
    tool(
        "read_file",
        "Read a UTF-8 workspace file as `N:hash|text`. Optional 1-based offset and limit.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "offset": {"type": "integer", "minimum": 1},
                "limit": {"type": "integer", "minimum": 1, "maximum": 1000}
            },
            "required": ["path"],
            "additionalProperties": false
        }),
    )
}

fn replace_text() -> ToolDefinition {
    tool(
        "replace_text",
        "Replace one exact substring in a file. When old occurs more than once, tag picks the line it starts on; a globally unique old is replaced directly and needs no tag. old and new may span lines. A stale or ambiguous edit fails before writing.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "tag": {"type": "string"},
                "old": {"type": "string"},
                "new": {"type": "string"}
            },
            "required": ["path", "old", "new"],
            "additionalProperties": false
        }),
    )
}

fn replace_texts() -> ToolDefinition {
    tool(
        "replace_texts",
        "Apply exact-substring replacements to one file atomically, in listed order. Each old may span lines and can match text produced by an earlier edit. When an old occurs more than once, tag picks the line it starts on; a unique old needs no tag. Any stale or ambiguous edit fails before writing.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "edits": {
                    "type": "array",
                    "minItems": 2,
                    "maxItems": 16,
                    "items": {
                        "type": "object",
                        "properties": {
                            "tag": {"type": "string"},
                            "old": {"type": "string"},
                            "new": {"type": "string"}
                        },
                        "required": ["old", "new"],
                        "additionalProperties": false
                    }
                }
            },
            "required": ["path", "edits"],
            "additionalProperties": false
        }),
    )
}

fn replace_lines() -> ToolDefinition {
    tool(
        "replace_lines",
        "Replace whole lines from tag through optional end_tag. Each tag must identify one line; use N:hash when text such as `}` repeats. new may span lines.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "tag": {"type": "string"},
                "end_tag": {"type": "string"},
                "new": {"type": "string"}
            },
            "required": ["path", "tag", "new"],
            "additionalProperties": false
        }),
    )
}

fn insert_after() -> ToolDefinition {
    tool(
        "insert_after",
        "Insert text after one tagged line. Use N:hash when the tagged line repeats. text may span lines.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "tag": {"type": "string"},
                "text": {"type": "string"}
            },
            "required": ["path", "tag", "text"],
            "additionalProperties": false
        }),
    )
}

fn delete_lines() -> ToolDefinition {
    tool(
        "delete_lines",
        "Delete whole lines from tag through optional end_tag. Each tag must identify one line; use N:hash when the line repeats.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "tag": {"type": "string"},
                "end_tag": {"type": "string"}
            },
            "required": ["path", "tag"],
            "additionalProperties": false
        }),
    )
}

fn write_file() -> ToolDefinition {
    tool(
        "write_file",
        "Create or overwrite a workspace file. Prefer an edit tool for existing code.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"}
            },
            "required": ["path", "content"],
            "additionalProperties": false
        }),
    )
}

fn list_dir() -> ToolDefinition {
    tool(
        "list_dir",
        "List a workspace directory. Skips .git, target, node_modules.",
        serde_json::json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "additionalProperties": false
        }),
    )
}

fn grep() -> ToolDefinition {
    tool(
        "grep",
        "Search workspace files. Each hit is `path:N:hash|line` so you can edit without another read. glob limits which files (*.rs, src/**/*.ts). Empty pattern with glob lists matching files. The workspace search honors .gitignore; an explicit path searches everything under it.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "path": {"type": "string"},
                "glob": {"type": "string"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 200}
            },
            "additionalProperties": false
        }),
    )
}

fn diagnostics() -> ToolDefinition {
    tool(
        "diagnostics",
        "Type errors and lint from the curated Rust, TypeScript/JavaScript, Python, or Go language module for this file. After edits, pass the file you changed.",
        serde_json::json!({
            "type": "object",
            "properties": {"path": {"type": "string"}},
            "required": ["path"],
            "additionalProperties": false
        }),
    )
}

fn definition() -> ToolDefinition {
    tool(
        "definition",
        "Go to definition. references true lists every use. new_name applies the language-server rename (denied in plan mode). path is a workspace file. line and character are 1-based (from read_file line numbers).",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "line": {"type": "integer", "minimum": 1},
                "character": {"type": "integer", "minimum": 1},
                "references": {"type": "boolean"},
                "new_name": {"type": "string"}
            },
            "required": ["path", "line"],
            "additionalProperties": false
        }),
    )
}

fn skill() -> ToolDefinition {
    tool(
        "skill",
        "Load a named skill body. Use after you pick a name from the skill catalog in the system prompt.",
        serde_json::json!({
            "type": "object",
            "properties": {"name": {"type": "string"}},
            "required": ["name"],
            "additionalProperties": false
        }),
    )
}

fn run_command() -> ToolDefinition {
    tool(
        "run_command",
        "Run a workspace shell command. Foreground commands default to 120 seconds; pass timeout_secs or set background true for longer tests. Background returns a job id and keeps running after Summoner exits. Pass job to poll it, or job plus kill true to stop it. Results contain the exit code and a concise diagnostic digest. Do not create or edit files with the shell; use write_file or edit_file. Prefer web_search for discovery and web_fetch for public pages.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {"type": "string"},
                "timeout_secs": {"type": "integer", "minimum": 1, "maximum": 600},
                "background": {"type": "boolean"},
                "job": {"type": "string"},
                "kill": {"type": "boolean"}
            },
            "additionalProperties": false
        }),
    )
}

fn web_search() -> ToolDefinition {
    tool(
        "web_search",
        "Search the live public web without an API key. Returns numbered title, URL, and snippet results. Use grep for workspace code. Call web_fetch to read a result. freshness: day, week, or month.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "count": {"type": "integer", "minimum": 1, "maximum": 8},
                "freshness": {"type": "string", "enum": ["day", "week", "month"]}
            },
            "required": ["query"],
            "additionalProperties": false
        }),
    )
}

fn web_fetch() -> ToolDefinition {
    tool(
        "web_fetch",
        "Fetch one public https URL as markdown. Use after web_search or when the user gave a URL. Truncated. Fails for private or authenticated pages.",
        serde_json::json!({
            "type": "object",
            "properties": {"url": {"type": "string"}},
            "required": ["url"],
            "additionalProperties": false
        }),
    )
}
