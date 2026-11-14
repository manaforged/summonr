use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ToolDefinition, tool};

/// The condition under which a built-in tool is advertised or runs.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gate {
    Always,
    Never,
    TaskList,
    Skills,
    SkillCatalog,
    Language,
    LspAvailable,
    Writable,
    Web,
    Browser,
    BrowserWritable,
}

#[derive(Deserialize)]
struct Spec {
    kind: Builtin,
    claude: Option<String>,
    #[serde(default)]
    claude_aliases: Vec<String>,
    acp_kind: String,
    verb: String,
    readonly: bool,
    advertise: Gate,
    run: Gate,
    description: Option<String>,
    help: Option<String>,
    schema: Option<Value>,
}

#[derive(Deserialize)]
struct Shape {
    description: String,
    schema: Value,
}

#[derive(Deserialize)]
struct Retired {
    name: String,
    live: Builtin,
    op: Option<String>,
}

#[derive(Deserialize)]
struct Data {
    tool: Vec<Spec>,
    apply_patch: Shape,
    retired: Vec<Retired>,
}

/// One row of the built-in tools table.
pub struct Tool {
    kind: Builtin,
    name: &'static str,
    claude: Option<&'static str>,
    claude_aliases: &'static [&'static str],
    acp_kind: &'static str,
    verb: &'static str,
    readonly: bool,
    pub advertise: Gate,
    pub run: Gate,
    help: Option<String>,
    pub definition: Option<ToolDefinition>,
}

/// The built-in tools table with the apply-patch definition and retired names.
pub struct Toolset {
    pub tool: Vec<Tool>,
    pub apply_patch: ToolDefinition,
    retired: Vec<(&'static str, Builtin, Option<&'static str>)>,
}

fn leak(text: String) -> &'static str {
    text.leak()
}

/// The built-in tools table, loaded once.
pub fn catalog() -> &'static Toolset {
    static LOADED: OnceLock<Toolset> = OnceLock::new();
    LOADED.get_or_init(|| {
        let data: Data = summonr_core::embedded!("tool", "tools.toml");
        let tools = data
            .tool
            .into_iter()
            .map(|spec| {
                let name = leak(
                    serde_json::to_value(spec.kind)
                        .ok()
                        .and_then(|name| name.as_str().map(str::to_string))
                        .unwrap_or_default(),
                );
                let definition = spec
                    .description
                    .zip(spec.schema)
                    .map(|(description, schema)| tool(name, &description, schema));
                Tool {
                    kind: spec.kind,
                    name,
                    claude: spec.claude.map(leak),
                    claude_aliases: spec
                        .claude_aliases
                        .into_iter()
                        .map(leak)
                        .collect::<Vec<_>>()
                        .leak(),
                    acp_kind: leak(spec.acp_kind),
                    verb: leak(spec.verb),
                    readonly: spec.readonly,
                    advertise: spec.advertise,
                    run: spec.run,
                    help: spec.help,
                    definition,
                }
            })
            .collect();
        Toolset {
            tool: tools,
            apply_patch: tool(
                "apply_patch",
                &data.apply_patch.description,
                data.apply_patch.schema,
            ),
            retired: data
                .retired
                .into_iter()
                .map(|row| (leak(row.name), row.live, row.op.map(leak)))
                .collect(),
        }
    })
}

/// The help text of a built-in tool.
pub fn help(kind: Builtin) -> String {
    kind.spec().help.clone().unwrap_or_default()
}

/// A built-in tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Builtin {
    Read,
    Search,
    Edit,
    Write,
    Lsp,
    Skill,
    RunCommand,
    Monitor,
    Todo,
    Web,
    Browser,
    Task,
}

impl Builtin {
    /// The built-in tool with the current name `name`.
    #[must_use]
    pub fn live(name: &str) -> Option<Self> {
        catalog()
            .tool
            .iter()
            .find(|tool| tool.name == name)
            .map(|tool| tool.kind)
    }

    /// The built-in tool that a Claude Code tool name or alias maps to.
    #[must_use]
    pub(crate) fn from_claude(name: &str) -> Option<Self> {
        let named = |claude: &&str| claude.eq_ignore_ascii_case(name);
        catalog()
            .tool
            .iter()
            .find(|tool| tool.claude.iter().chain(tool.claude_aliases).any(named))
            .map(|tool| tool.kind)
    }

    /// This tool's row in the built-in tools table.
    pub fn spec(self) -> &'static Tool {
        match catalog().tool.iter().find(|tool| tool.kind == self) {
            Some(tool) => tool,
            None => panic!("the built-in tools table has no {self:?} row"),
        }
    }

    /// This tool's current name.
    #[must_use]
    pub fn name(self) -> &'static str {
        self.spec().name
    }

    /// This tool's Claude Code name and aliases.
    #[must_use]
    pub fn claude(self) -> (Option<&'static str>, &'static [&'static str]) {
        (self.spec().claude, self.spec().claude_aliases)
    }

    /// This tool's Agent Client Protocol tool kind.
    #[must_use]
    pub fn acp_kind(self) -> &'static str {
        self.spec().acp_kind
    }

    /// The built-in tool for a current or retired name.
    #[must_use]
    pub fn of(name: &str) -> Option<Self> {
        retired(name).map_or_else(|| Self::live(name), |row| Some(row.1))
    }

    /// Whether `name` is a current or retired name of `kind`.
    #[must_use]
    pub fn is(name: &str, kind: Self) -> bool {
        Self::of(name) == Some(kind)
    }

    /// Whether this tool only reads.
    #[must_use]
    pub fn readonly(self) -> bool {
        self.spec().readonly
    }

    /// Whether this tool edits or writes files.
    #[must_use]
    pub fn edits(self) -> bool {
        matches!(self, Self::Edit | Self::Write)
    }

    /// The verb that describes a call of this tool.
    #[must_use]
    pub fn verb(self) -> &'static str {
        self.spec().verb
    }
}

/// The retired-name row for `name`: the name, its tool, and its operation.
pub fn retired(name: &str) -> Option<&'static (&'static str, Builtin, Option<&'static str>)> {
    catalog()
        .retired
        .iter()
        .find(|(retired, _, _)| *retired == name)
}

/// The operation a retired tool name maps to.
pub fn retired_op(name: &str) -> Option<&'static str> {
    retired(name).and_then(|(_, _, op)| *op)
}

/// Whether `name` names a built-in tool that only reads.
pub fn readonly(name: &str) -> bool {
    Builtin::of(name).is_some_and(Builtin::readonly)
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
