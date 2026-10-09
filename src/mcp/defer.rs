use std::sync::Arc;

use serde_json::{Value, json};

use super::Hub;
use super::link::Link;
use super::tool::Tool;
use crate::error::{Error, Result};
use crate::policy::Policy;
use crate::provider::ToolDefinition;
use crate::tools::tool;

pub(crate) const SEARCH_TOOL: &str = "mcp";

pub(crate) enum Resolved {
    Found(String),
    Call { alias: String, arguments: Value },
}

pub(crate) fn arguments(name: &str, text: &str) -> Result<Value> {
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(text)
        .map_err(|error| Error::Tool(format!("{name} arguments are invalid: {error}")))
}

fn routed(value: Value) -> Result<std::result::Result<(String, Value), String>> {
    if let Some(alias) = value.get("tool").and_then(Value::as_str) {
        let alias = alias.to_string();
        return Ok(Ok((alias, call_arguments(value))));
    }
    value
        .get("query")
        .and_then(Value::as_str)
        .map(|query| Err(query.to_string()))
        .ok_or_else(|| {
            Error::Tool(
                "mcp takes `query` to find tools, or `tool` and `arguments` to call one"
                    .to_string(),
            )
        })
}

fn call_arguments(mut value: Value) -> Value {
    match value.get_mut("arguments").map(Value::take) {
        Some(Value::String(text)) => serde_json::from_str(&text).unwrap_or_else(|_| json!({})),
        Some(Value::Null) | None => {
            if let Some(fields) = value.as_object_mut() {
                fields.remove("tool");
                fields.remove("arguments");
            }
            value
        }
        Some(arguments) => arguments,
    }
}

pub(crate) fn effective(name: &str, text: &str) -> Option<(String, Value)> {
    if name != SEARCH_TOOL {
        return None;
    }
    routed(arguments(name, text).ok()?).ok()?.ok()
}

const LABEL_ARG_WIDTH: usize = 72;

#[must_use]
pub fn call_label(name: &str, arguments: &str) -> String {
    effective(name, arguments).map_or_else(
        || {
            let summary = crate::tui::tool_arg(arguments);
            if summary.is_empty() {
                name.to_string()
            } else {
                format!(
                    "{name} {}",
                    crate::text::ellipsize(&summary, LABEL_ARG_WIDTH)
                )
            }
        },
        |(alias, _)| {
            let target = alias.split_once("__").map_or_else(
                || alias.clone(),
                |(server, tool)| format!("{server}:{tool}"),
            );
            format!("{name} {target}")
        },
    )
}

fn allowed<'a>(link: &'a Arc<Link>, policy: &'a Policy) -> impl Iterator<Item = &'a Tool> {
    link.tools
        .iter()
        .filter(|item| policy.tool(&item.alias, item.readonly).is_ok())
}

fn declared(item: &Tool) -> ToolDefinition {
    tool(&item.alias, &item.description, item.schema.clone())
}

fn deferred(link: &Arc<Link>, policy: &Policy) -> bool {
    allowed(link, policy)
        .map(|item| serde_json::to_vec(&declared(item)).map_or(0, |bytes| bytes.len()))
        .sum::<usize>()
        > crate::limits::limits().mcp_defer_bytes
}

impl Hub {
    pub(crate) fn resolve(&self, name: &str, text: &str, policy: &Policy) -> Result<Resolved> {
        let value = arguments(name, text)?;
        if name != SEARCH_TOOL {
            return Ok(Resolved::Call {
                alias: name.to_string(),
                arguments: value,
            });
        }
        Ok(match routed(value)? {
            Ok((alias, arguments)) => Resolved::Call { alias, arguments },
            Err(query) => Resolved::Found(self.search(&query, policy)),
        })
    }

    pub(crate) fn definitions(&self, policy: &Policy) -> Vec<ToolDefinition> {
        let mut definitions = Vec::new();
        let mut catalog = Vec::new();
        for link in &self.links {
            if deferred(link, policy) {
                let names: Vec<&str> = allowed(link, policy)
                    .map(|item| item.alias.as_str())
                    .collect();
                catalog.push(format!("{}: {}", link.name(), names.join(", ")));
            } else {
                definitions.extend(allowed(link, policy).map(declared));
            }
        }
        if !catalog.is_empty() {
            definitions.push(search_definition(&catalog));
        }
        definitions
    }

    fn search(&self, query: &str, policy: &Policy) -> String {
        let words: Vec<String> = query
            .split(|c: char| !c.is_alphanumeric())
            .filter(|word| !word.is_empty())
            .map(str::to_lowercase)
            .collect();
        let mut scored: Vec<(usize, &Tool)> = self
            .links
            .iter()
            .flat_map(|link| allowed(link, policy))
            .filter_map(|item| {
                let text = format!("{} {}", item.alias, item.description).to_lowercase();
                let score = if item.alias == query.trim() {
                    usize::MAX
                } else {
                    words.iter().filter(|word| text.contains(*word)).count()
                };
                (score > 0).then_some((score, item))
            })
            .collect();
        scored.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        if scored.is_empty() {
            return format!("no MCP tool matches `{query}`");
        }
        scored
            .iter()
            .take(crate::limits::limits().mcp_found_max)
            .map(|(_, item)| {
                json!({
                    "tool": item.alias,
                    "description": item.description,
                    "input_schema": item.schema,
                })
                .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn search_definition(catalog: &[String]) -> ToolDefinition {
    let description = format!(
        "Find and call tools from MCP servers whose schemas are not listed. Pass `query` (words or a tool name) to get matching tools with their input schemas, then pass `tool` and `arguments` to call one. Servers and tools:\n{}",
        catalog.join("\n")
    );
    tool(
        SEARCH_TOOL,
        &description,
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Words or a tool name to look up"},
                "tool": {"type": "string", "description": "The tool to call, as server__tool"},
                "arguments": {"type": "object", "description": "Arguments for the tool, matching its input schema"}
            }
        }),
    )
}
