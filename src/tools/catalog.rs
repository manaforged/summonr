use serde::Deserialize;
use serde_json::Value;
use summonr_types::catalog::{Gate, catalog};

use crate::provider::ToolDefinition;

use super::ToolRegistry;
use summonr_core::error::{Error, Result};

pub(super) use summonr_types::catalog::retired;
pub(crate) use summonr_types::catalog::{Builtin, readonly, retired_op};

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

    pub(in crate::tools) fn set_browser(&mut self, on: bool) -> Result<()> {
        self.browser = if on {
            Some(match self.browser.take() {
                Some(held) => held,
                None => crate::browser::Browser::discover()?,
            })
        } else {
            None
        };
        Ok(())
    }

    pub(in crate::tools) async fn browse(&self, arguments: &str) -> Result<String> {
        let session = self.session.lock().ok().and_then(|held| held.clone());
        let browser = self
            .browser
            .as_ref()
            .ok_or_else(|| Error::Tool("the browser module is off".to_string()))?;
        let resolve = |path: &str| self.resolve_read(path, crate::tools::paths::Shield::Contained);
        browser.call(arguments, &resolve, session.as_deref()).await
    }

    pub(crate) fn definitions(&self) -> Vec<ToolDefinition> {
        catalog()
            .tool
            .iter()
            .filter(|spec| self.allows(spec.advertise))
            .filter_map(|spec| spec.definition.clone())
            .map(shaped_tool)
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
            Gate::Writable => !self.policy.is_read_only() || self.policy.shell_allowed(),
            Gate::Web => self.search.is_some(),
            Gate::Browser => self.browser.is_some(),
            Gate::BrowserWritable => self.browser.is_some() && !self.policy.is_read_only(),
        }
    }
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

fn shaped_tool(mut definition: ToolDefinition) -> ToolDefinition {
    fill_schema(&mut definition.function.parameters);
    definition.function.description = fill_text(&definition.function.description);
    definition
}

pub(crate) fn shaped(definitions: Vec<ToolDefinition>, patch: bool) -> Vec<ToolDefinition> {
    let mut shaped = definitions;
    if patch {
        shaped.push(shaped_tool(catalog().apply_patch.clone()));
    }
    shaped
}

pub(super) fn help(kind: Builtin) -> String {
    fill_text(&summonr_types::catalog::help(kind))
}

#[cfg(test)]
#[path = "catalog/patch_tests.rs"]
mod tests;
