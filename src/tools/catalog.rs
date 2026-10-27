use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::provider::ToolDefinition;

use super::{ToolRegistry, tool};

mod browser;
mod patch;
pub(crate) use patch::shaped;

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
        catalog()
            .tool
            .iter()
            .filter(|spec| self.allows(spec.advertise))
            .filter_map(|spec| spec.definition.clone())
            .collect()
    }

    pub(super) fn enabled(&self, kind: Builtin) -> bool {
        self.allows(kind.spec().run)
    }

    fn probed(&self) -> (bool, bool) {
        *self.probed.get_or_init(|| {
            (
                self.language && super::lsp::available(&self.workspace),
                self.skills && crate::skills::catalog(&self.workspace).is_some(),
            )
        })
    }

    fn allows(&self, gate: Gate) -> bool {
        match gate {
            Gate::Always => true,
            Gate::Never => false,
            Gate::TaskList => self.task_list,
            Gate::Skills => self.skills,
            Gate::SkillCatalog => self.probed().1,
            Gate::Language => self.language,
            Gate::LspAvailable => self.probed().0,
            Gate::Writable => !self.policy.is_read_only(),
            Gate::Web => self.search.is_some(),
            Gate::Browser => self.browser.is_some(),
            Gate::BrowserWritable => self.browser.is_some() && !self.policy.is_read_only(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Gate {
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

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Fill {
    ReadLineLimit,
    ReadPageKib,
    CommandWaitSecs,
    SearchSecs,
    SearchContextMax,
    SearchDefault,
    SearchMax,
}

impl Fill {
    fn value(self) -> u64 {
        let limits = crate::limits::limits();
        let count = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
        match self {
            Self::ReadLineLimit => count(limits.read_line_limit),
            Self::ReadPageKib => count(super::budget::SHAPED_BUDGET / 1024),
            Self::CommandWaitSecs => limits.command_wait_secs,
            Self::SearchSecs => limits.search_secs,
            Self::SearchContextMax => count(super::workspace::search::CONTEXT_MAX),
            Self::SearchDefault => count(super::workspace::search::GREP_DEFAULT),
            Self::SearchMax => count(super::workspace::search::GREP_MAX),
        }
    }
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

struct Tool {
    kind: Builtin,
    name: &'static str,
    claude: Option<&'static str>,
    claude_aliases: &'static [&'static str],
    acp_kind: &'static str,
    verb: &'static str,
    readonly: bool,
    advertise: Gate,
    run: Gate,
    help: Option<String>,
    definition: Option<ToolDefinition>,
}

struct Catalog {
    tool: Vec<Tool>,
    apply_patch: ToolDefinition,
    retired: Vec<(&'static str, Builtin, Option<&'static str>)>,
}

fn leak(text: String) -> &'static str {
    text.leak()
}

fn fill_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let token = after.find('}').map(|close| &after[..close]);
        match token
            .and_then(|token| serde_json::from_value::<Fill>(Value::String(token.to_string())).ok())
        {
            Some(fill) => {
                out.push_str(&fill.value().to_string());
                rest = &after[token.map_or(0, str::len) + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn fill_schema(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.len() == 1
                && let Some(fill) = map
                    .get("fill")
                    .and_then(|token| serde_json::from_value::<Fill>(token.clone()).ok())
            {
                *value = Value::from(fill.value());
                return;
            }
            map.values_mut().for_each(fill_schema);
        }
        Value::Array(items) => items.iter_mut().for_each(fill_schema),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => {}
    }
}

fn shaped_tool(name: &str, description: &str, mut schema: Value) -> ToolDefinition {
    fill_schema(&mut schema);
    tool(name, &fill_text(description), schema)
}

fn catalog() -> &'static Catalog {
    static LOADED: OnceLock<Catalog> = OnceLock::new();
    LOADED.get_or_init(|| {
        let data: Data = match toml::from_str(include_str!("tools.toml")) {
            Ok(data) => data,
            Err(error) => panic!("the built-in tools table is invalid: {error}"),
        };
        let tool = data
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
                    .map(|(description, schema)| shaped_tool(name, &description, schema));
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
                    help: spec.help.as_deref().map(fill_text),
                    definition,
                }
            })
            .collect();
        Catalog {
            tool,
            apply_patch: shaped_tool(
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

pub(super) fn apply_patch() -> ToolDefinition {
    catalog().apply_patch.clone()
}

pub(super) fn help(kind: Builtin) -> String {
    kind.spec().help.clone().unwrap_or_default()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
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

impl Builtin {
    #[must_use]
    pub(crate) fn live(name: &str) -> Option<Self> {
        catalog()
            .tool
            .iter()
            .find(|tool| tool.name == name)
            .map(|tool| tool.kind)
    }

    #[must_use]
    pub(crate) fn from_claude(name: &str) -> Option<Self> {
        let named = |claude: &&str| claude.eq_ignore_ascii_case(name);
        catalog()
            .tool
            .iter()
            .find(|tool| tool.claude.iter().chain(tool.claude_aliases).any(named))
            .map(|tool| tool.kind)
    }

    fn spec(self) -> &'static Tool {
        match catalog().tool.iter().find(|tool| tool.kind == self) {
            Some(tool) => tool,
            None => panic!("the built-in tools table has no {self:?} row"),
        }
    }

    #[must_use]
    pub(crate) fn name(self) -> &'static str {
        self.spec().name
    }

    #[must_use]
    pub(crate) fn claude(self) -> (Option<&'static str>, &'static [&'static str]) {
        (self.spec().claude, self.spec().claude_aliases)
    }

    #[must_use]
    pub(crate) fn acp_kind(self) -> &'static str {
        self.spec().acp_kind
    }

    #[must_use]
    pub(crate) fn of(name: &str) -> Option<Self> {
        retired(name).map_or_else(|| Self::live(name), |row| Some(row.1))
    }

    #[must_use]
    pub(crate) fn is(name: &str, kind: Self) -> bool {
        Self::of(name) == Some(kind)
    }

    #[must_use]
    pub(crate) fn readonly(self) -> bool {
        self.spec().readonly
    }

    #[must_use]
    pub(crate) fn edits(self) -> bool {
        matches!(self, Self::Edit | Self::Write)
    }

    #[must_use]
    pub(crate) fn verb(self) -> &'static str {
        self.spec().verb
    }
}

pub(super) fn retired(
    name: &str,
) -> Option<&'static (&'static str, Builtin, Option<&'static str>)> {
    catalog()
        .retired
        .iter()
        .find(|(retired, _, _)| *retired == name)
}

pub(crate) fn retired_op(name: &str) -> Option<&'static str> {
    retired(name).and_then(|(_, _, op)| *op)
}

pub(crate) fn readonly(name: &str) -> bool {
    Builtin::of(name).is_some_and(Builtin::readonly)
}
