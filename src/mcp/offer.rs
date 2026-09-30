use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use super::catalog;
use super::config;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum OfferKind {
    Prompt,
    Resource,
    Template,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Offer {
    pub(crate) kind: OfferKind,
    pub(crate) insert: String,
    pub(crate) about: String,
}

#[must_use]
pub(crate) fn offers(workspace: &Path) -> Vec<Offer> {
    let Ok(found) = config::servers(workspace) else {
        return Vec::new();
    };
    found
        .servers
        .iter()
        .filter_map(|server| catalog::load(&server.key).map(|listing| (server, listing)))
        .flat_map(|(server, listing)| {
            let name = &server.name;
            let prompts = listing.prompts.iter().filter_map(move |item| {
                Some(Offer {
                    kind: OfferKind::Prompt,
                    insert: format!("/{name}:{}", text(item, "name")?),
                    about: [usage(item), text(item, "description").unwrap_or_default()]
                        .into_iter()
                        .filter(|part| !part.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · "),
                })
            });
            let resources = listing.resources.iter().filter_map(move |item| {
                Some(Offer {
                    kind: OfferKind::Resource,
                    insert: format!("@{name}:{}", text(item, "uri")?),
                    about: about(item),
                })
            });
            let templates = listing.templates.iter().filter_map(move |item| {
                Some(Offer {
                    kind: OfferKind::Template,
                    insert: format!("@{name}:{}", text(item, "uriTemplate")?),
                    about: about(item),
                })
            });
            prompts
                .chain(resources)
                .chain(templates)
                .collect::<Vec<_>>()
        })
        .collect()
}

fn text(item: &Value, key: &str) -> Option<String> {
    item.get(key).and_then(Value::as_str).map(str::to_string)
}

fn about(item: &Value) -> String {
    text(item, "title")
        .or_else(|| text(item, "name"))
        .or_else(|| text(item, "description"))
        .unwrap_or_default()
}

pub(super) fn usage(prompt: &Value) -> String {
    prompt
        .get("arguments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|argument| {
            let name = argument.get("name")?.as_str()?;
            Some(
                if argument.get("required").and_then(Value::as_bool) == Some(true) {
                    format!("<{name}>")
                } else {
                    format!("[{name}]")
                },
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}
