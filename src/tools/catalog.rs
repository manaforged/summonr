use crate::provider::ToolDefinition;

use super::{ToolRegistry, tool};

impl ToolRegistry {
    pub(crate) fn definitions(&self) -> Vec<ToolDefinition> {
        if self.policy.is_read_only() {
            let mut all = vec![read()];
            if self.skill_ready() {
                all.push(skill());
            }
            all.push(run_command());
            if self.search.is_some() {
                all.push(web());
            }
            return all;
        }
        let mut all = vec![read(), edit(), write()];
        if self.skill_ready() {
            all.push(skill());
        }
        if self.language && super::lsp::available(&self.workspace) {
            all.push(lsp());
        }
        all.push(run_command());
        if self.search.is_some() {
            all.push(web());
        }
        all
    }

    fn skill_ready(&self) -> bool {
        self.skills && crate::skills::catalog(&self.workspace).is_some()
    }

    pub(super) fn enabled(&self, name: &str) -> bool {
        match name {
            "read" | "read_file" | "list_dir" | "grep" | "show_image" => true,
            "skill" => self.skills,
            "lsp" | "diagnostics" | "definition" => self.language,
            "edit" | "edit_file" | "replace_text" | "replace_texts" | "replace_lines"
            | "insert_after" | "delete_lines" | "write" | "write_file" => {
                !self.policy.is_read_only()
            }
            "run_command" => true,
            "web" | "web_search" | "web_fetch" => self.search.is_some(),
            _ => false,
        }
    }
}

pub(crate) fn readonly(name: &str) -> bool {
    matches!(
        name,
        "read"
            | "read_file"
            | "list_dir"
            | "grep"
            | "show_image"
            | "diagnostics"
            | "skill"
            | "web"
            | "web_search"
            | "web_fetch"
    )
}

pub(super) fn known(name: &str) -> bool {
    matches!(
        name,
        "read"
            | "read_file"
            | "edit"
            | "edit_file"
            | "replace_text"
            | "replace_texts"
            | "replace_lines"
            | "insert_after"
            | "delete_lines"
            | "write"
            | "write_file"
            | "list_dir"
            | "grep"
            | "show_image"
            | "lsp"
            | "diagnostics"
            | "definition"
            | "skill"
            | "run_command"
            | "web"
            | "web_search"
            | "web_fetch"
    )
}

fn read() -> ToolDefinition {
    tool(
        "read",
        "Read a file as N:hash|text (offset/limit), list a directory, show an image, or search (pattern/glob). Hits are path:N:hash|line.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "offset": {"type": "integer", "minimum": 1},
                "limit": {"type": "integer", "minimum": 1, "maximum": 1000},
                "pattern": {"type": "string"},
                "glob": {"type": "string"}
            },
            "additionalProperties": false
        }),
    )
}

fn edit() -> ToolDefinition {
    tool(
        "edit",
        "Change an existing file. Tags are N:hash from read. old+new replaces a unique substring; tag+new replaces lines through optional end_tag; tag+text inserts after; op=delete removes tagged lines; edits applies several atomically. Empty files: write.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "old": {"type": "string"},
                "new": {"type": "string"},
                "tag": {"type": "string"},
                "end_tag": {"type": "string"},
                "text": {"type": "string"},
                "op": {"type": "string", "enum": ["delete"]},
                "edits": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "old": {"type": "string"},
                            "new": {"type": "string"},
                            "tag": {"type": "string"},
                            "end_tag": {"type": "string"},
                            "text": {"type": "string"},
                            "op": {"type": "string", "enum": ["delete"]}
                        },
                        "additionalProperties": false
                    }
                }
            },
            "required": ["path"],
            "additionalProperties": false
        }),
    )
}

fn write() -> ToolDefinition {
    tool(
        "write",
        "Create or overwrite a file.",
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

fn lsp() -> ToolDefinition {
    tool(
        "lsp",
        "Language server. path for diagnostics; path+line for definition; references true lists uses; new_name renames.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "line": {"type": "integer", "minimum": 1},
                "character": {"type": "integer", "minimum": 1},
                "references": {"type": "boolean"},
                "new_name": {"type": "string"}
            },
            "required": ["path"],
            "additionalProperties": false
        }),
    )
}

fn run_command() -> ToolDefinition {
    tool(
        "run_command",
        "Run a shell command in the workspace sandbox. Network is off unless the user runs /net on or /net once. background true returns a job id; pass job to poll, kill true to stop. Output is the exit code and a digest.",
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

fn web() -> ToolDefinition {
    tool(
        "web",
        "Search the public web (query) or fetch one https URL as markdown (url). freshness: day, week, or month.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "url": {"type": "string"},
                "count": {"type": "integer", "minimum": 1, "maximum": 8},
                "freshness": {"type": "string", "enum": ["day", "week", "month"]}
            },
            "additionalProperties": false
        }),
    )
}
