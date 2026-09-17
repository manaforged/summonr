use crate::provider::ToolDefinition;

use super::{ToolRegistry, tool};

impl ToolRegistry {
    pub(crate) fn definitions(&self) -> Vec<ToolDefinition> {
        if self.policy.is_read_only() {
            let mut all = vec![read()];
            if self.skill_ready() {
                all.push(skill());
            }
            all.push(run_command(!self.denies_network()));
            all.push(todo());
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
        all.push(run_command(!self.denies_network()));
        all.push(todo());
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
            "read" => true,
            "skill" => self.skills,
            "lsp" => self.language,
            "edit" | "write" => !self.policy.is_read_only(),
            "run_command" | "todo" => true,
            "web" => self.search.is_some(),
            _ => false,
        }
    }
}

/// Retired names that still dispatch, and the row each one answers to. Not
/// advertised. One spelling per row: a synonym is a defect.
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

/// The advertised name for any name the dispatcher accepts.
#[must_use]
pub(crate) fn canonical(name: &str) -> &str {
    RETIRED
        .iter()
        .find(|(retired, _, _)| *retired == name)
        .map_or(name, |(_, live, _)| live)
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
        "Read a file as N:hash|text (offset/limit), list a directory, show an image, or search (pattern/glob). Hits are path:N:hash|line. literal skips regex; caseSensitive defaults true.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "offset": {"type": "integer", "minimum": 1},
                "limit": {"type": "integer", "minimum": 1, "maximum": 1000},
                "pattern": {"type": "string"},
                "glob": {"type": "string"},
                "include": {"type": "string"},
                "literal": {"type": "boolean"},
                "caseSensitive": {"type": "boolean"},
                "op": {"type": "string", "enum": ["file", "list", "search", "image"]}
            },
            "additionalProperties": false
        }),
    )
}

fn edit() -> ToolDefinition {
    tool(
        "edit",
        "Change an existing file, or move one with path and to. Tags are N:hash from read. old+new replaces a unique substring; tag+new replaces lines through optional end_tag; tag+text inserts after; op=delete removes tagged lines; edits applies several atomically; op=sequence resolves edits in order; files applies several files atomically. Empty files: write.",
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
                "op": {"type": "string", "enum": ["delete", "sequence"]},
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
                },
                "files": {
                    "type": "array",
                    "minItems": 1,
                    "items": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"},
                            "to": {"type": "string"},
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
        "Replace the Work todo list (itemized steps). status is pending, active, or done. Not a plan document.",
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
        "Language server. path for diagnostics; path+line for definition; references true lists uses; new_name renames.",
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

fn run_command(network: bool) -> ToolDefinition {
    let net = if network {
        "Network is on. /net off denies sockets."
    } else {
        "Network is off. /net on allows sockets."
    };
    tool(
        "run_command",
        &format!(
            "Run a shell command in the workspace sandbox. {net} workdir picks the directory. A command stops after {wait}s unless timeout_secs sets a longer limit. background true returns a job id; pass job to poll, kill true to stop. Output is the exit code and a digest; a long log is kept at the path the output names. For a password use `sudo -A` or plain ssh; the user types it in a masked prompt. `summonr askpass \"<label>\"` prints a user-typed secret for piping into a command such as `ssh host sudo -S`; never echo or store it.",
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
                "kill": {"type": "boolean"}
            },
            "additionalProperties": false
        }),
    )
}

fn web() -> ToolDefinition {
    tool(
        "web",
        "Search the public web (query) or fetch one https URL as markdown, text, or html (url). format defaults to markdown. freshness: day, week, or month.",
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
