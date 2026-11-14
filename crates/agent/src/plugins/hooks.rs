use serde_json::Value;

use super::HOOKS;
use crate::hooks::{Hook, HookEvent, Hooks, Matcher};

pub(super) fn handlers(doc: &Value) -> impl Iterator<Item = (HookEvent, &Value, &Value)> {
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

pub(super) fn claude(doc: &Value) -> Hooks {
    let mut hooks = Hooks::default();
    for (event, group, handler) in handlers(doc) {
        let matcher = group["matcher"].as_str().and_then(Matcher::parse);
        if let Some(hook) = Hook::claude(handler, matcher) {
            hooks.push(event, hook);
        }
    }
    hooks
}
