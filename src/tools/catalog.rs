use crate::provider::ToolDefinition;

use super::{ToolRegistry, tool};

impl ToolRegistry {
    pub(crate) fn definitions(&self) -> Vec<ToolDefinition> {
        let mut all = vec![
            read_file(),
            edit_file(),
            write_file(),
            list_dir(),
            grep(),
            diagnostics(),
            definition(),
            skill(),
            run_command(),
        ];
        if self.search.is_some() {
            all.extend([web_search(), web_fetch()]);
        }
        all
    }

    pub(super) fn enabled(&self, name: &str) -> bool {
        match name {
            "read_file" | "list_dir" | "grep" | "diagnostics" | "definition" | "skill" => true,
            "edit_file" | "write_file" | "run_command" => !self.policy.is_read_only(),
            "web_search" | "web_fetch" => self.search.is_some(),
            _ => false,
        }
    }
}

pub(super) fn known(name: &str) -> bool {
    matches!(
        name,
        "read_file"
            | "edit_file"
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
                "limit": {"type": "integer", "minimum": 1, "maximum": 400}
            },
            "required": ["path"],
            "additionalProperties": false
        }),
    )
}

fn edit_file() -> ToolDefinition {
    tool(
        "edit_file",
        "Edit by hashline. One edit: replace {tag, new, end_tag?} (new may be multiline), insert_after {tag, text}, or delete {tag, end_tag?}. Several independent edits to one file: edits [ {replace|insert_after|delete}, … ]; every anchor resolves against the original file and overlapping edits fail before writing. Prefer the hash alone when unique; N:hash breaks ties. A stale or ambiguous tag fails before any write and shows current tags. A successful write returns fresh tags; any post-edit diagnostic is advisory.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "replace": {
                    "type": "object",
                    "properties": {
                        "tag": {"type": "string"},
                        "end_tag": {"type": "string"},
                        "new": {"type": "string"}
                    },
                    "required": ["tag", "new"],
                    "additionalProperties": false
                },
                "insert_after": {
                    "type": "object",
                    "properties": {
                        "tag": {"type": "string"},
                        "text": {"type": "string"}
                    },
                    "required": ["tag", "text"],
                    "additionalProperties": false
                },
                "delete": {
                    "type": "object",
                    "properties": {
                        "tag": {"type": "string"},
                        "end_tag": {"type": "string"}
                    },
                    "required": ["tag"],
                    "additionalProperties": false
                },
                "edits": {
                    "type": "array",
                    "items": {"type": "object"},
                    "description": "Several independent edits to this file. Every anchor resolves against the original file; overlapping edits fail before writing."
                }
            },
            "required": ["path"],
            "additionalProperties": false
        }),
    )
}

fn write_file() -> ToolDefinition {
    tool(
        "write_file",
        "Create or overwrite a workspace file. Prefer edit_file for existing code.",
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
        "Search workspace files. Each hit is `path:N:hash|line` so you can edit without another read. glob limits which files (*.rs, src/**/*.ts). Empty pattern with glob lists matching files.",
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
        "Type errors and lint from the workspace language server. After edits, pass the file you changed. Plug in Biome, ESLint LSP, Ruff, rust-analyzer, … with [lsp] command in .mage/config.toml.",
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
        "Run a workspace shell command. Foreground commands default to 30 seconds; pass timeout_secs or set background true for longer tests. Background returns a job id and keeps running after Summoner exits. Pass job to poll it, or job plus kill true to stop it. Results contain the exit code and a concise diagnostic digest. Do not create or edit files with the shell; use write_file or edit_file. Prefer web_search for discovery and web_fetch for public pages.",
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
