use crate::provider::ToolDefinition;

use super::{ToolRegistry, tool};

mod browser;
mod patch;
mod search;
use browser::browser;
pub(crate) use patch::shaped;
use search::search;

impl ToolRegistry {
    pub(crate) fn set_modules(&mut self, switches: &crate::modules::Switches) -> crate::Result<()> {
        use crate::modules::ModuleId;
        use crate::web::Search;

        let search = if switches.enabled(ModuleId::Search) {
            match &self.search {
                Some(search) => Some(search.clone()),
                None => Some(Search::discover()?),
            }
        } else {
            None
        };
        self.set_browser(switches.enabled(ModuleId::Browser))?;
        self.search = search;
        if let Some(search) = &self.search {
            let renderer = if switches.enabled(ModuleId::Browser) {
                Some(crate::browser::Browser::ephemeral()?)
            } else {
                None
            };
            search.set_browser(renderer);
        }
        self.language = switches.enabled(ModuleId::Lsp);
        self.background = switches.enabled(ModuleId::Jobs);
        self.skills = switches.enabled(ModuleId::Skills);
        self.probed = std::sync::OnceLock::new();
        Ok(())
    }

    pub(crate) fn definitions(&self) -> Vec<ToolDefinition> {
        let (lsp_on, skill_on) = *self.probed.get_or_init(|| {
            (
                self.language && super::lsp::available(&self.workspace),
                self.skills && crate::skills::catalog(&self.workspace).is_some(),
            )
        });
        let mut all = vec![read(), search(), edit(), write()];
        if skill_on {
            all.push(skill());
        }
        if lsp_on {
            all.push(lsp());
        }
        all.push(run_command());
        if self.search.is_some() {
            all.push(web());
        }
        if self.browser.is_some() {
            all.push(browser());
        }
        if self.task_list {
            all.push(todo());
        }
        all
    }

    pub(super) fn enabled(&self, kind: Builtin) -> bool {
        match kind {
            Builtin::Read | Builtin::Search | Builtin::Edit | Builtin::Write => true,
            Builtin::Todo => self.task_list,
            Builtin::Skill => self.skills,
            Builtin::Lsp => self.language,
            Builtin::RunCommand => !self.policy.is_read_only(),
            Builtin::Web => self.search.is_some(),
            Builtin::Browser => self.browser.is_some() && !self.policy.is_read_only(),
            Builtin::Task => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Builtin {
    Read,
    Search,
    Edit,
    Write,
    Lsp,
    Skill,
    RunCommand,
    Todo,
    Web,
    Browser,
    Task,
}

#[rustfmt::skip]
type Row = (&'static str, Builtin, Option<&'static str>, &'static [&'static str], &'static str, &'static str, bool);

#[rustfmt::skip]
const LIVE: &[Row] = &[
    ("read", Builtin::Read, Some("Read"), &[], "read", "Read", true),
    ("search", Builtin::Search, Some("Grep"), &["Glob"], "search", "Search", true),
    ("edit", Builtin::Edit, Some("Edit"), &["MultiEdit", "NotebookEdit"], "edit", "Edit", false),
    ("write", Builtin::Write, Some("Write"), &[], "edit", "Write", false),
    ("lsp", Builtin::Lsp, None, &["LSP"], "search", "Lsp", true),
    ("skill", Builtin::Skill, Some("Skill"), &[], "other", "Skill", true),
    ("run_command", Builtin::RunCommand, Some("Bash"), &[], "execute", "Run", false),
    ("todo", Builtin::Todo, Some("TodoWrite"), &[], "other", "Tasks", true),
    ("web", Builtin::Web, Some("WebFetch"), &["WebSearch"], "fetch", "Web", true),
    ("browser", Builtin::Browser, None, &[], "fetch", "Browse", false),
    ("task", Builtin::Task, Some("Task"), &[], "other", "Subagent", false),
];

impl Builtin {
    #[must_use]
    pub(crate) fn live(name: &str) -> Option<Self> {
        LIVE.iter().find(|row| row.0 == name).map(|row| row.1)
    }

    #[must_use]
    pub(crate) fn from_claude(name: &str) -> Option<Self> {
        let named = |claude: &&str| claude.eq_ignore_ascii_case(name);
        LIVE.iter()
            .find(|row| row.2.iter().chain(row.3).any(named))
            .map(|row| row.1)
    }

    fn row(self) -> &'static Row {
        &LIVE[self as usize]
    }

    #[must_use]
    pub(crate) fn claude(self) -> (Option<&'static str>, &'static [&'static str]) {
        (self.row().2, self.row().3)
    }

    #[must_use]
    pub(crate) fn acp_kind(self) -> &'static str {
        self.row().4
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
        self.row().6
    }

    #[must_use]
    pub(crate) fn edits(self) -> bool {
        matches!(self, Self::Edit | Self::Write)
    }

    #[must_use]
    pub(crate) fn verb(self) -> &'static str {
        self.row().5
    }
}

pub(crate) const RETIRED: &[(&str, &str, Option<&str>)] = &[
    ("read_file", "read", Some("file")),
    ("list_dir", "read", Some("list")),
    ("grep", "search", None),
    ("glob", "search", None),
    ("show_image", "read", Some("image")),
    ("apply_patch", "edit", None),
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
        "Read files (N:text; offset/limit, widened by before/after/context; paths[] for several; tags true adds hash tags for tag-edits), list directories as a tree, or view an image file. op=help returns the full reference.",
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "offset": {"type": "integer", "minimum": 1},
                "limit": {"type": "integer", "minimum": 1, "maximum": crate::limits::limits().read_line_limit},
                "paths": {"type": "array", "items": {"type": "string"}},
                "before": {"type": "integer", "minimum": 0, "maximum": 10},
                "after": {"type": "integer", "minimum": 0, "maximum": 10},
                "context": {"type": "integer", "minimum": 0, "maximum": 10},
                "tags": {"type": "boolean"},
                "recursive": {"type": "boolean"},
                "depth": {"type": "integer", "minimum": 1},
                "op": {"type": "string", "enum": ["file", "list", "image", "help"]}
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
        "Create or overwrite one file (path and content), or several at once with files.",
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
            "additionalProperties": false
        }),
    )
}

fn todo() -> ToolDefinition {
    tool(
        "todo",
        "Maintain the Work task list; status pending|active|done. Update statuses as tasks start or finish, not just when making the initial plan. A done item does not replace the completion check.",
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
        "Language server. Prefer over scanning files for definitions and uses. path+line+character jumps to the definition (character is the 1-based column a search hit prints as path:N:col), references true lists uses, new_name renames, path alone returns diagnostics.",
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
            "Run builds, tests, and command-line tools in the workspace; use edit/write for source changes and search for files or text. After {wait}s a command continues as a job unless timeout_secs stops it; background returns a job id, job waits on one, kill stops it. op=help returns output, job, and policy details.",
            wait = crate::limits::limits().command_wait_secs
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
