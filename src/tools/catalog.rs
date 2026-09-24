use crate::provider::ToolDefinition;

use super::{ToolRegistry, tool};

impl ToolRegistry {
    pub(crate) fn definitions(&self) -> Vec<ToolDefinition> {
        let (lsp_on, skill_on) = *self.probed.get_or_init(|| {
            (
                self.language && super::lsp::available(&self.workspace),
                self.skills && crate::skills::catalog(&self.workspace).is_some(),
            )
        });
        let writes = !self.policy.is_read_only();
        let mut all = vec![read()];
        if writes {
            all.extend([edit(), write()]);
        }
        if skill_on {
            all.push(skill());
        }
        if lsp_on && writes {
            all.push(lsp());
        }
        if self.enabled("run_command") {
            all.push(run_command());
        }
        all.push(todo());
        if self.search.is_some() {
            all.push(web());
        }
        all
    }

    pub(super) fn enabled(&self, name: &str) -> bool {
        match name {
            "read" => true,
            "skill" => self.skills,
            "lsp" => self.language,
            "edit" | "write" => !self.policy.is_read_only(),
            "run_command" => !self.policy.is_read_only() || self.sandbox.is_some(),
            "todo" => true,
            "web" => self.search.is_some(),
            _ => false,
        }
    }
}

pub(crate) const RETIRED: &[(&str, &str, Option<&str>)] = &[
    ("read_file", "read", Some("file")),
    ("list_dir", "read", Some("list")),
    ("grep", "read", Some("search")),
    ("show_image", "read", Some("image")),
    ("edit_file", "edit", None),
    ("replace_text", "edit", None),
    ("replace_texts", "edit", Some("sequence")),
    ("replace_lines", "edit", None),
    ("insert_after", "edit", None),
    ("delete_lines", "edit", Some("delete")),
    ("write_file", "write", None),
    ("diagnostics", "lsp", Some("diagnostics")),
    ("definition", "lsp", Some("definition")),
    ("web_search", "web", Some("search")),
    ("web_fetch", "web", Some("fetch")),
];

#[must_use]
pub(crate) fn canonical(name: &str) -> &str {
    RETIRED
        .iter()
        .find(|(retired, _, _)| *retired == name)
        .map_or(name, |(_, live, _)| live)
}

pub(crate) fn retired_op(name: &str) -> Option<&'static str> {
    RETIRED
        .iter()
        .find(|(retired, _, _)| *retired == name)
        .and_then(|(_, _, op)| *op)
}

pub(crate) fn readonly(name: &str) -> bool {
    matches!(canonical(name), "read" | "lsp" | "skill" | "todo" | "web")
}

pub(super) fn known(name: &str) -> bool {
    matches!(
        name,
        "read" | "edit" | "write" | "lsp" | "skill" | "run_command" | "todo" | "web"
    )
}

fn read() -> ToolDefinition {
    tool(
        "read",
        "Read files (N:text; offset/limit; paths[] for several; tags true adds hash tags for tag-edits), list directories as a tree honoring ignore rules, show an image (op=image), or search contents (pattern with glob/type/exclude/context; hits path:N:line; tags true adds hashes). op=help returns the full reference.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "offset": {"type": "integer", "minimum": 1},
                "limit": {"type": "integer", "minimum": 1, "maximum": 1000},
                "pattern": {"type": "string"},
                "glob": {"type": "string"},
                "literal": {"type": "boolean"},
                "caseSensitive": {"type": "boolean"},
                "paths": {"type": "array", "items": {"type": "string"}},
                "files": {"type": "boolean"},
                "count": {"type": "boolean"},
                "type": {"type": "string"},
                "exclude": {"type": "string"},
                "before": {"type": "integer", "minimum": 0, "maximum": 10},
                "after": {"type": "integer", "minimum": 0, "maximum": 10},
                "context": {"type": "integer", "minimum": 0, "maximum": 10},
                "no_ignore": {"type": "boolean"},
                "multiline": {"type": "boolean"},
                "tags": {"type": "boolean"},
                "recursive": {"type": "boolean"},
                "depth": {"type": "integer", "minimum": 1},
                "op": {"type": "string", "enum": ["file", "list", "search", "image", "help"]}
            },
            "additionalProperties": false
        }),
    )
}

fn edit() -> ToolDefinition {
    tool(
        "edit",
        "Change an existing file (path+to moves it). old+new replaces a unique substring, tag+new replaces tagged lines (tags come from a read with tags true), tag+text inserts after, edits[] applies several atomically, patch takes a *** Begin Patch document. op=help returns the full grammar. New or empty files: write.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "to": {"type": "string"},
                "old": {"type": "string"},
                "new": {"type": "string"},
                "tag": {"type": "string"},
                "end_tag": {"type": "string"},
                "text": {"type": "string"},
                "replace_all": {"type": "boolean"},
                "patch": {"type": "string"},
                "op": {"type": "string", "enum": ["delete", "sequence", "help"]},
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
                            "replace_all": {"type": "boolean"},
                            "op": {"type": "string", "enum": ["delete"]}
                        },
                        "additionalProperties": false
                    }
                }
            },
            "additionalProperties": false
        }),
    )
}

fn write() -> ToolDefinition {
    tool(
        "write",
        "Create or overwrite one file, or several at once with files.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"},
                "files": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"},
                            "content": {"type": "string"}
                        },
                        "required": ["path", "content"],
                        "additionalProperties": false
                    }
                }
            },
            "anyOf": [
                {"required": ["path", "content"]},
                {"required": ["files"]}
            ],
            "additionalProperties": false
        }),
    )
}

fn todo() -> ToolDefinition {
    tool(
        "todo",
        "Replace the Work todo list; status pending|active|done.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "text": { "type": "string" },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "active", "done"]
                            }
                        },
                        "required": ["text"]
                    }
                }
            },
            "required": ["items"],
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
        "Language server — prefer over scanning files for definitions and uses. path+line jumps to the definition, references true lists uses, new_name renames, path alone returns diagnostics.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "line": {"type": "integer", "minimum": 1},
                "character": {"type": "integer", "minimum": 1},
                "references": {"type": "boolean"},
                "op": {"type": "string", "enum": ["diagnostics", "definition"]},
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
        &format!(
            "Run a shell command in the workspace sandbox. workdir picks the directory; timeout_secs extends the {wait}s default; background returns a job to poll, kill stops it. op=help returns output and policy details.",
            wait = crate::runtime::JOB_WAIT_SECS
        ),
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {"type": "string"},
                "workdir": {"type": "string"},
                "timeout_secs": {"type": "integer", "minimum": 1},
                "background": {"type": "boolean"},
                "job": {"type": "string"},
                "kill": {"type": "boolean"},
                "op": {"type": "string", "enum": ["help"]}
            },
            "additionalProperties": false
        }),
    )
}

fn web() -> ToolDefinition {
    tool(
        "web",
        "Search the public web (query) or fetch one https URL (url; format markdown|text|html; freshness day|week|month).",
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"},
                "url": {"type": "string"},
                "count": {"type": "integer", "minimum": 1, "maximum": 8},
                "freshness": {"type": "string", "enum": ["day", "week", "month"]},
                "op": {"type": "string", "enum": ["search", "fetch"]},
                "format": {"type": "string", "enum": ["markdown", "text", "html"]}
            },
            "additionalProperties": false
        }),
    )
}
