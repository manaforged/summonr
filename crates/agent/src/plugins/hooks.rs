use serde_json::Value;

use super::HOOKS;
use crate::hooks::{Hook, HookEvent, Hooks, Matcher};

fn handlers(doc: &Value) -> impl Iterator<Item = (HookEvent, &Value, &Value)> {
    HookEvent::ALL.into_iter().flat_map(move |event| {
        doc[HOOKS][event.claude()]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(move |group| {
                group[HOOKS]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(move |handler| (event, group, handler))
            })
    })
}

/// The hooks in a Claude Code document's `hooks` table, as `hooks.json` and `settings.json` hold it.
pub(crate) fn claude(doc: &Value) -> Hooks {
    let mut hooks = Hooks::default();
    for (event, group, handler) in handlers(doc) {
        let matcher = group["matcher"].as_str().and_then(Matcher::parse);
        if let Some(hook) = Hook::claude(handler, matcher) {
            hooks.push(event, hook);
        }
    }
    hooks
}

/// Each hook event and handler type in `doc` that summonr does not run, named for a note.
pub(crate) fn unsupported(doc: &Value) -> Vec<String> {
    let events = doc[HOOKS]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(name, _)| name)
        .filter(|name| {
            !HookEvent::ALL
                .iter()
                .any(|event| event.claude() == name.as_str())
        })
        .map(|name| format!("hook event `{name}`"));
    let handlers =
        skipped(doc).map(|(event, kind)| format!("`{kind}` hook handler on {}", event.claude()));
    events.chain(handlers).collect()
}

/// Each handler in `doc` that is not a command, as its event and its `type`.
pub(super) fn skipped(doc: &Value) -> impl Iterator<Item = (HookEvent, &str)> {
    handlers(doc)
        .filter(|(_, _, handler)| handler["type"] != "command")
        .map(|(event, _, handler)| (event, handler["type"].as_str().unwrap_or("hook")))
}

/// What makes `doc` a hook document summonr cannot read, or `None` when its shape is sound.
pub(crate) fn shape(doc: &Value) -> Option<String> {
    let Value::Object(file) = doc else {
        return (!doc.is_null()).then(|| "the file is not a JSON object".to_string());
    };
    let table = match file.get(HOOKS) {
        None => return None,
        Some(Value::Object(table)) => table,
        Some(_) => return Some(format!("`{HOOKS}` is not an object")),
    };
    for event in HookEvent::ALL {
        let name = event.claude();
        let Some(groups) = table.get(name) else {
            continue;
        };
        let Some(groups) = groups.as_array() else {
            return Some(format!("`{HOOKS}.{name}` is not a list"));
        };
        for (group, entry) in groups.iter().enumerate() {
            let Some(handlers) = entry[HOOKS].as_array() else {
                return Some(format!("`{HOOKS}.{name}[{group}].{HOOKS}` is not a list"));
            };
            for (index, handler) in handlers.iter().enumerate() {
                if handler["type"] == "command" && !handler["command"].is_string() {
                    return Some(format!(
                        "`{HOOKS}.{name}[{group}].{HOOKS}[{index}].command` is not a string"
                    ));
                }
            }
        }
    }
    None
}
