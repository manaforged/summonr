use crate::provider::ToolDefinition;

use super::{ToolRegistry, tool};

impl ToolRegistry {
    pub(crate) fn definitions(&self) -> Vec<ToolDefinition> {
        if self.policy.is_read_only() {
            let mut all = vec![run_command()];
            if self.skill_ready() {
                all.insert(0, skill());
            }
            if self.search.is_some() {
                all.extend([web_search(), web_fetch()]);
            }
            return all;
        }
        let mut all = vec![read_file()];
        all.extend([
            replace_text(),
            replace_texts(),
            replace_lines(),
            insert_after(),
            delete_lines(),
            write_file(),
        ]);
        all.extend([list_dir(), grep()]);
        if self.skill_ready() {
            all.push(skill());
        }
        if self.language && super::lsp::available(&self.workspace) {
            all.extend([diagnostics(), definition()]);
        }
        all.push(run_command());
        if self.search.is_some() {
            all.extend([web_search(), web_fetch()]);
        }
        all
    }

    fn skill_ready(&self) -> bool {
        self.skills && crate::skills::catalog(&self.workspace).is_some()
    }

    pub(super) fn enabled(&self, name: &str) -> bool {
        match name {
            "read_file" | "list_dir" | "grep" => true,
            "skill" => self.skills,
            "diagnostics" | "definition" => self.language,
            "edit_file" | "replace_text" | "replace_texts" | "replace_lines" | "insert_after"
            | "delete_lines" | "write_file" => !self.policy.is_read_only(),
            "run_command" => true,
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
        "Read a workspace file as `N:hash|text` lines. Optional 1-based offset and limit.",
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
        "Replace one exact substring. A unique old needs no tag; otherwise tag names the line old starts on. Stale or ambiguous edits fail before writing.",
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
        "Apply several exact-substring replacements to one file atomically, in order. Later olds may match text an earlier edit produced. Tag rules as replace_text.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "edits": {
                    "type": "array",
                    "minItems": 2,
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
        "Replace whole lines from tag through optional end_tag. Use N:hash when a line's text repeats.",
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
        "Insert text after the tagged line. Use N:hash when the line repeats.",
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
        "Delete whole lines from tag through optional end_tag.",
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
        "Create or overwrite a file. Prefer an edit tool for existing code.",
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
        "Search workspace files. Hits are `path:N:hash|line`, ready to use as edit tags. glob limits files (*.rs, src/**/*.ts); an empty pattern with glob lists files.",
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
        "Type errors and lint for one file from its language server. Run after edits.",
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
        "Go to definition at line:character (1-based). references true lists every use; new_name renames.",
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
        "Load a skill body by name from the catalog in the system prompt.",
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
        "Run a shell command in the workspace sandbox; network is off unless the user enables it. Use edit tools for source changes. background true returns a job id: pass job to poll it, kill true to stop it. Output is the exit code and a digest.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {"type": "string"},
                "timeout_secs": {"type": "integer", "minimum": 1},
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
        "Search the public web. Returns numbered title, URL, and snippet. freshness: day, week, or month.",
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
        "Fetch one public https URL as markdown, truncated.",
        serde_json::json!({
            "type": "object",
            "properties": {"url": {"type": "string"}},
            "required": ["url"],
            "additionalProperties": false
        }),
    )
}
