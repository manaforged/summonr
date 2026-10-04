use crate::WorkKind;
use crate::provider::ToolDefinition;

use super::{ToolRegistry, tool};

impl ToolRegistry {
    pub(crate) fn definitions(&self, work: WorkKind) -> Vec<ToolDefinition> {
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
        if self.enabled(Builtin::RunCommand) {
            all.push(run_command());
        }
        if matches!(work, WorkKind::Steps | WorkKind::Goal) {
            all.push(todo());
        }
        if self.search.is_some() {
            all.push(web());
        }
        all
    }

    pub(super) fn enabled(&self, kind: Builtin) -> bool {
        match kind {
            Builtin::Read | Builtin::Todo => true,
            Builtin::Skill => self.skills,
            Builtin::Lsp => self.language,
            Builtin::Edit | Builtin::Write => !self.policy.is_read_only(),
            Builtin::RunCommand => !self.policy.is_read_only() || self.sandbox.is_some(),
            Builtin::Web => self.search.is_some(),
            Builtin::Task => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Builtin {
    Read,
    Edit,
    Write,
    Lsp,
    Skill,
    RunCommand,
    Todo,
    Web,
    Task,
}

const LIVE: &[(&str, Builtin)] = &[
    ("read", Builtin::Read),
    ("edit", Builtin::Edit),
    ("write", Builtin::Write),
    ("lsp", Builtin::Lsp),
    ("skill", Builtin::Skill),
    ("run_command", Builtin::RunCommand),
    ("todo", Builtin::Todo),
    ("web", Builtin::Web),
    ("task", Builtin::Task),
];

impl Builtin {
    #[must_use]
    pub(crate) fn live(name: &str) -> Option<Self> {
        LIVE.iter()
            .find(|(live, _)| *live == name)
            .map(|(_, kind)| *kind)
    }

    #[must_use]
    pub(crate) fn of(name: &str) -> Option<Self> {
        Self::live(canonical(name))
    }

    #[must_use]
    pub(crate) fn is(name: &str, kind: Self) -> bool {
        Self::of(name) == Some(kind)
    }

    #[must_use]
    pub(crate) fn readonly(self) -> bool {
        matches!(
            self,
            Self::Read | Self::Lsp | Self::Skill | Self::Todo | Self::Web
        )
    }

    #[must_use]
    pub(crate) fn edits(self) -> bool {
        matches!(self, Self::Edit | Self::Write)
    }

    #[must_use]
    pub(crate) fn verb(self) -> &'static str {
        match self {
            Self::Read => "Read",
            Self::Edit => "Edit",
            Self::Write => "Write",
            Self::Lsp => "Lsp",
            Self::Skill => "Skill",
            Self::RunCommand => "Run",
            Self::Todo => "Todo",
            Self::Web => "Web",
            Self::Task => "Task",
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
fn canonical(name: &str) -> &str {
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
    Builtin::of(name).is_some_and(Builtin::readonly)
}

fn read() -> ToolDefinition {
    tool(
        "read",
        "Read files (N:text; offset/limit, widened by before/after/context; paths[] for several; tags true adds hash tags for tag-edits), list directories as a tree, view an image file, or search contents (pattern, glob, context; hits path:N:col:line). op=help returns the full reference.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "offset": {"type": "integer", "minimum": 1},
                "limit": {"type": "integer", "minimum": 1, "maximum": crate::limits::READ_LINE_LIMIT},
                "pattern": {"type": "string"},
                "glob": {"type": "string"},
                "paths": {"type": "array", "items": {"type": "string"}},
                "before": {"type": "integer", "minimum": 0, "maximum": 10},
                "after": {"type": "integer", "minimum": 0, "maximum": 10},
                "context": {"type": "integer", "minimum": 0, "maximum": 10},
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
        "Change an existing file. Use old+new: copy old exactly from the file, including indentation; it must be unique. edits[] applies several at once, in order if they overlap. Tags (from read with tags true) are only for when old is not unique. op=help covers moves, ranges, and patches. New or empty files: write.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "old": {"type": "string"},
                "new": {"type": "string"},
                "tag": {"type": "string"},
                "text": {"type": "string"},
                "replace_all": {"type": "boolean"},
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
        "Language server — prefer over scanning files for definitions and uses. path+line+character jumps to the definition (character is the 1-based column a read search hit prints as path:N:col), references true lists uses, new_name renames, path alone returns diagnostics.",
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
            "Run a shell command in the workspace sandbox. workdir picks the directory; after {wait}s a command continues as a job unless timeout_secs sets a hard stop; background returns a job to wait on, kill stops it. op=help returns output and policy details.",
            wait = crate::limits::COMMAND_WAIT_SECS
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
