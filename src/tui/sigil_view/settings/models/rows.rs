use std::collections::BTreeMap;

use crate::catalog::Model;
use crate::endpoint::State;
use crate::recent::ModelId;

const CURRENT: &str = "current";

pub(super) enum ModelRow {
    Header(String),
    Provider(String, usize),
    Model(usize),
    Fix(usize),
    Older(usize),
}

pub(super) fn list_rows(
    choices: &[ModelId],
    counts: [usize; 3],
    groups: &BTreeMap<String, State>,
    aged: usize,
) -> Vec<ModelRow> {
    let mut rows = Vec::with_capacity(choices.len() + 8);
    let (mut attention, mut setup) = (Vec::new(), Vec::new());
    let mut start = 0;
    for (title, count) in [CURRENT, "favorites", "recent"].into_iter().zip(counts) {
        let end = (start + count).min(choices.len());
        if end > start {
            let title = if rows.is_empty() && title == CURRENT {
                format!("{title} · {}", choices[start].endpoint)
            } else {
                title.to_string()
            };
            rows.push(ModelRow::Header(title));
            rows.extend((start..end).map(ModelRow::Model));
        }
        start = end;
    }
    let mut heading = "";
    for (index, choice) in choices.iter().enumerate().skip(start) {
        let name = choice.endpoint.as_str();
        match groups.get(name).copied().filter(|state| state.blocked()) {
            Some(state) if state.failed() => attention.push(ModelRow::Fix(index)),
            Some(_) => setup.push(ModelRow::Fix(index)),
            None => {
                if name != heading {
                    heading = name;
                    let count = choices[index..]
                        .iter()
                        .take_while(|selection| selection.endpoint == name)
                        .count();
                    rows.push(ModelRow::Provider(name.to_string(), count));
                }
                rows.push(ModelRow::Model(index));
            }
        }
    }
    if aged > 0 {
        rows.push(ModelRow::Older(aged));
    }
    for (title, fixes) in [("needs attention", attention), ("not set up", setup)] {
        if !fixes.is_empty() {
            rows.push(ModelRow::Header(title.to_string()));
            rows.extend(fixes);
        }
    }
    rows
}

fn label<'a>(selection: &'a ModelId, facts: &'a BTreeMap<ModelId, Model>) -> &'a str {
    facts
        .get(selection)
        .map_or(selection.model.as_str(), Model::label)
}

pub(super) fn names(choices: &[ModelId], facts: &BTreeMap<ModelId, Model>) -> Vec<String> {
    let mut seen: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for selection in choices {
        *seen
            .entry((selection.endpoint.as_str(), label(selection, facts)))
            .or_default() += 1;
    }
    choices
        .iter()
        .map(|selection| {
            let name = label(selection, facts);
            if seen
                .get(&(selection.endpoint.as_str(), name))
                .copied()
                .unwrap_or(0)
                > 1
            {
                selection.model.clone()
            } else {
                name.to_string()
            }
        })
        .collect()
}

pub(super) fn fix(name: &str, state: State, cause: Option<&String>) -> Option<String> {
    let cause = cause.and_then(|cause| cause.lines().next());
    Some(match state {
        State::NeedsKey => format!("↵ add a key for {name}"),
        State::NeedsModel => format!("{name} needs a model in config.toml"),
        State::SignIn => format!("↵ sign in to {name}"),
        State::KeyRejected => format!("{name} rejected the key. ↵ replaces the key"),
        State::RateLimited => format!("{name} is rate limited. Models still usable; F5 retries"),
        State::Offline => format!("{name} did not answer. ↵ retries"),
        State::Error => format!("{name}: {}", cause.unwrap_or(state.word())),
        State::Ready | State::NoKey | State::Checking | State::Loading => return None,
    })
}

pub(super) const fn action(state: State) -> &'static str {
    match state {
        State::NeedsKey => "↵ set a key",
        State::SignIn => "↵ sign in",
        State::KeyRejected => "↵ new key",
        State::NeedsModel => "set a model in config.toml",
        State::Offline | State::Error | State::RateLimited => "↵ retry",
        State::Ready | State::NoKey | State::Checking | State::Loading => "↵ use",
    }
}

#[cfg(test)]
#[path = "rows_tests.rs"]
mod tests;
