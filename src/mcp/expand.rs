use std::sync::Arc;

use serde_json::{Map, Value, json};

use super::link::Link;
use super::offer::usage;
use super::output::output;
use super::{Hub, McpUpdate};
use crate::error::{Error, Result};
use crate::provider::{Source, UserInput};
use crate::tools::sandbox::Sandbox;

impl Hub {
    pub(crate) async fn expand(
        &self,
        input: UserInput,
        sandbox: Option<&Sandbox>,
    ) -> Result<UserInput> {
        let original = input.text.clone();
        let mut text = input.text;
        let mut images = input.images;
        let before = images.len();
        if let Some((link, name, rest)) = self.prompt_of(&original) {
            let (body, found) = prompt(link, &name, &rest, sandbox).await?;
            text = body;
            images.extend(found);
        }
        let mut attached = String::new();
        for word in original.split_whitespace() {
            let Some((link, uri)) = self.resource_of(word) else {
                continue;
            };
            let (body, found) = resource(link, uri, sandbox).await?;
            attached.push_str(&format!("\n--- {} ---\n{body}\n", &word[1..]));
            images.extend(found);
        }
        if text == original && attached.is_empty() && images.len() == before {
            return Ok(UserInput {
                text,
                images,
                source: input.source,
            });
        }
        let shown = input.source.map_or(original, |source| source.text);
        Ok(UserInput {
            text: format!("{text}{attached}"),
            source: Some(Source {
                text: shown,
                images: images.len(),
            }),
            images,
        })
    }

    fn link(&self, name: &str) -> Option<&Arc<Link>> {
        self.links.iter().find(|link| link.name() == name)
    }

    fn prompt_of(&self, text: &str) -> Option<(&Arc<Link>, String, String)> {
        let text = text.trim_start();
        let (head, rest) = text.split_once(char::is_whitespace).unwrap_or((text, ""));
        let (server, name) = head.strip_prefix('/')?.split_once(':')?;
        Some((
            self.link(server)?,
            name.to_string(),
            rest.trim().to_string(),
        ))
    }

    fn resource_of<'a>(&self, word: &'a str) -> Option<(&Arc<Link>, &'a str)> {
        let (server, uri) = word.strip_prefix('@')?.split_once(':')?;
        (!uri.is_empty()).then_some(())?;
        Some((self.link(server)?, uri))
    }
}

pub(crate) fn wanted(text: &str) -> bool {
    let head = text.split_whitespace().next().unwrap_or_default();
    (head.starts_with('/') && head.contains(':'))
        || text
            .split_whitespace()
            .any(|word| word.starts_with('@') && word.contains(':'))
}

async fn prompt(
    link: &Arc<Link>,
    name: &str,
    rest: &str,
    sandbox: Option<&Sandbox>,
) -> Result<(String, Vec<crate::provider::Image>)> {
    let listing = link.listing();
    let declared = listing
        .prompts
        .iter()
        .find(|item| item.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| Error::Tool(format!("mcp {} has no prompt `{name}`", link.name())))?;
    let arguments = bind(declared, rest).map_err(|missing| {
        Error::Tool(format!(
            "/{}:{name} needs {missing}; usage: /{}:{name} {}",
            link.name(),
            link.name(),
            usage(declared)
        ))
    })?;
    let mut result = link
        .request(
            "prompts/get",
            json!({"name": name, "arguments": arguments}),
            sandbox,
            &mut |_: McpUpdate| {},
        )
        .await?;
    let content: Vec<Value> = match result.get_mut("messages").map(Value::take) {
        Some(Value::Array(messages)) => messages
            .into_iter()
            .filter_map(|mut message| message.get_mut("content").map(Value::take))
            .collect(),
        Some(_) | None => Vec::new(),
    };
    let shown = output(json!({"content": content}), false);
    Ok((shown.text, shown.images))
}

fn bind(declared: &Value, rest: &str) -> std::result::Result<Map<String, Value>, String> {
    let names: Vec<(&str, bool)> = declared
        .get("arguments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|argument| {
            Some((
                argument.get("name")?.as_str()?,
                argument.get("required").and_then(Value::as_bool) == Some(true),
            ))
        })
        .collect();
    let mut words = crate::policy::shell_words(rest);
    if names.is_empty() {
        return Ok(Map::new());
    }
    if words.len() > names.len() {
        let tail = words.split_off(names.len() - 1).join(" ");
        words.push(tail);
    }
    let mut bound = Map::new();
    for (index, (name, required)) in names.iter().enumerate() {
        match words.get(index) {
            Some(word) => drop(bound.insert((*name).to_string(), json!(word))),
            None if *required => return Err(format!("`{name}`")),
            None => {}
        }
    }
    Ok(bound)
}

async fn resource(
    link: &Arc<Link>,
    uri: &str,
    sandbox: Option<&Sandbox>,
) -> Result<(String, Vec<crate::provider::Image>)> {
    let mut result = link
        .request(
            "resources/read",
            json!({"uri": uri}),
            sandbox,
            &mut |_: McpUpdate| {},
        )
        .await?;
    let content: Vec<Value> = match result.get_mut("contents").map(Value::take) {
        Some(Value::Array(contents)) => contents.into_iter().map(part).collect(),
        Some(_) | None => Vec::new(),
    };
    let shown = output(json!({"content": content}), false);
    Ok((shown.text, shown.images))
}

fn part(mut item: Value) -> Value {
    let kind = item
        .get("mimeType")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if let Some(text) = item.get_mut("text").map(Value::take) {
        return json!({"type": "text", "text": text});
    }
    match item.get_mut("blob").map(Value::take) {
        Some(data) if kind.starts_with("image/") => {
            json!({"type": "image", "data": data, "mimeType": kind})
        }
        Some(_) | None => json!({"type": if kind.is_empty() { "binary" } else { kind.as_str() }}),
    }
}
