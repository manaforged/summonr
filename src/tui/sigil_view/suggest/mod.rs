use super::app::App;
use super::canvas;
use super::model::Model;
use crate::mcp::OfferKind;
use crate::tui::matching_commands;
use crate::{SlashCommand, parse_command};
use sigil::prelude::{Signal, signal};

const LIMIT: usize = 9;
const MAX_WIDTH: usize = 72;

type Rows = Vec<(String, String)>;
type Facts = std::collections::BTreeMap<crate::recent::Seat, crate::catalog::Model>;

#[derive(Clone, Copy)]
pub(super) struct Picker {
    cursor: Signal<usize>,
    hidden: Signal<bool>,
    query: Signal<String>,
    args: Signal<std::rc::Rc<(String, Rows)>>,
}

impl Picker {
    pub(super) fn new() -> Self {
        Self {
            cursor: signal(0),
            hidden: signal(false),
            query: signal(String::new()),
            args: signal(std::rc::Rc::default()),
        }
    }

    pub(super) fn change(
        self,
        draft: &str,
        levels: &[crate::provider::Effort],
        endpoint: &str,
        facts: &Facts,
    ) {
        self.cursor.set(0);
        self.hidden.set(false);
        self.query.set(draft.to_string());
        let head = draft.split_once(' ').map_or("", |(head, _)| head);
        if self.args.peek().0 != head {
            let rows = arguments(head, endpoint, levels, facts);
            self.args.set(std::rc::Rc::new((head.to_string(), rows)));
        }
    }

    pub(super) fn mentions(self) -> bool {
        self.query.peek().contains('@')
    }

    pub(super) fn open(self, draft: &str, caret: usize, model: &Model) -> bool {
        !self.hidden.get() && !self.rows(draft, caret, model).is_empty()
    }

    pub(super) fn hide(self, draft: &str, caret: usize, model: &Model) -> bool {
        if !self.open(draft, caret, model) {
            return false;
        }
        self.hidden.set(true);
        true
    }

    pub(super) fn shift(self, draft: &str, caret: usize, model: &Model, delta: isize) -> bool {
        let count = self.rows(draft, caret, model).len();
        if count == 0 || self.hidden.peek() {
            return false;
        }
        let cursor = self.cursor.peek().min(count - 1);
        let next = if delta < 0 {
            let amount = delta.unsigned_abs() % count;
            (cursor + count - amount) % count
        } else {
            (cursor + delta as usize) % count
        };
        self.cursor.set(next);
        true
    }

    pub(super) fn accept(self, draft: &str, caret: usize, model: &Model) -> Option<(String, bool)> {
        let command = self.head(draft).is_none() && mention(draft, caret).is_none();
        if self.hidden.peek() || (command && draft.ends_with(' ')) {
            return None;
        }
        let rows = self.rows(draft, caret, model);
        let row = rows.get(self.cursor.peek().min(rows.len().saturating_sub(1)))?;
        let command = command && !prompt_row(model, &row.0);
        (row.0 != draft).then(|| (row.0.clone(), command))
    }

    pub(super) fn complete(
        self,
        draft: &str,
        caret: usize,
        model: &Model,
        facts: &Facts,
    ) -> Option<String> {
        let rows = self.rows(draft, caret, model);
        if rows.is_empty() || self.hidden.peek() {
            return None;
        }
        let mut cursor = self.cursor.peek().min(rows.len() - 1);
        if rows[cursor].0 == draft && !usage(draft).is_empty() {
            let rows = arguments(
                draft,
                &model.snapshot.endpoint,
                &model.snapshot.levels,
                facts,
            );
            self.args.set(std::rc::Rc::new((draft.to_string(), rows)));
            return Some(format!("{draft} "));
        }
        if rows[cursor].0 == draft && rows.len() > 1 {
            cursor = (cursor + 1) % rows.len();
            self.cursor.set(cursor);
        }
        Some(rows[cursor].0.clone())
    }

    fn head(self, draft: &str) -> Option<&str> {
        let (head, typed) = draft.split_once(' ')?;
        let held = self.args.get();
        (held.0 == head && !held.1.is_empty() && !typed.contains(char::is_whitespace))
            .then_some(head)
    }

    fn cursor(self, count: usize) -> usize {
        self.cursor.get().min(count.saturating_sub(1))
    }

    fn rows(self, draft: &str, caret: usize, model: &Model) -> Vec<(String, String)> {
        if let Some((_, query)) = mention(draft, caret) {
            let query = query.to_lowercase();
            let mut peers: Vec<_> = model
                .peers
                .iter()
                .filter_map(|agent| {
                    let score = sigil::prelude::fuzzy_score(&query, agent.label())?.score;
                    Some((score, agent))
                })
                .collect();
            peers.sort_unstable_by_key(|(score, _)| *score);
            let mut matches: Vec<_> = model
                .files
                .iter()
                .filter_map(|path| {
                    let score = sigil::prelude::fuzzy_score(&query, path)?.score;
                    Some((score, path))
                })
                .collect();
            matches.sort_unstable();
            return peers
                .into_iter()
                .map(|(_, agent)| {
                    (
                        agent.label().to_string(),
                        format!("{} · {} · {}", agent.kind, agent.model, agent.status),
                    )
                })
                .chain(resources(model, &query))
                .chain(
                    matches
                        .into_iter()
                        .map(|(_, path)| (path.clone(), String::new())),
                )
                .collect();
        }
        if let Some(head) = self.head(draft) {
            let typed = draft[head.len()..].trim_start().to_lowercase();
            let mut ranked: Vec<_> = self
                .args
                .get()
                .1
                .iter()
                .filter_map(|(value, about)| {
                    let score = sigil::prelude::fuzzy_score(&typed, value)?.score;
                    Some((score, format!("{head} {value}"), about.clone()))
                })
                .collect();
            ranked.sort_by_key(|(score, ..)| *score);
            return ranked
                .into_iter()
                .map(|(_, value, about)| (value, about))
                .collect();
        }
        let query = self.query.get();
        let exact = crate::command_specs().iter().any(|spec| spec.name == draft);
        let rows = if exact && !query.is_empty() {
            matching_commands(&query)
        } else {
            matching_commands(draft)
        };
        rows.into_iter()
            .filter(|(name, _)| match parse_command(name) {
                Some(SlashCommand::PlanAccept | SlashCommand::PlanReject) => {
                    model.snapshot.pending_plan
                }
                Some(SlashCommand::Auto) => model.snapshot.plan,
                Some(SlashCommand::Plan) => model.prefs.plan_mode,
                _ => true,
            })
            .map(|(name, help)| (name.to_string(), help.to_string()))
            .chain(prompts(model, draft))
            .collect()
    }
}

fn prompts(model: &Model, draft: &str) -> Rows {
    if draft.contains(char::is_whitespace) {
        return Vec::new();
    }
    model
        .offers
        .iter()
        .filter(|offer| offer.kind == OfferKind::Prompt && offer.insert.starts_with(draft))
        .map(|offer| (format!("{} ", offer.insert), offer.about.clone()))
        .collect()
}

fn prompt_row(model: &Model, value: &str) -> bool {
    model.offers.iter().any(|offer| {
        offer.kind == OfferKind::Prompt && value.strip_suffix(' ') == Some(offer.insert.as_str())
    })
}

fn resources(model: &Model, query: &str) -> Rows {
    model
        .offers
        .iter()
        .filter(|offer| matches!(offer.kind, OfferKind::Resource | OfferKind::Template))
        .filter_map(|offer| {
            let name = offer.insert.strip_prefix('@').unwrap_or(&offer.insert);
            name.to_lowercase()
                .contains(query)
                .then(|| (name.to_string(), offer.about.clone()))
        })
        .collect()
}

pub(super) fn offered(model: &Model, value: &str) -> bool {
    model.offers.iter().any(|offer| {
        matches!(offer.kind, OfferKind::Resource | OfferKind::Template)
            && offer.insert.strip_prefix('@') == Some(value)
    })
}

fn usage(name: &str) -> &'static str {
    crate::command_specs()
        .iter()
        .find(|spec| spec.name == name)
        .map_or("", |spec| spec.usage)
}

fn arguments(
    head: &str,
    endpoint: &str,
    levels: &[crate::provider::Effort],
    facts: &Facts,
) -> Rows {
    let plain = |names: Vec<String>| {
        names
            .into_iter()
            .map(|name| (name, String::new()))
            .collect()
    };
    match parse_command(head).filter(|_| !usage(head).is_empty()) {
        Some(SlashCommand::Model) => {
            let mut held: Vec<(String, crate::catalog::Model)> = if facts.is_empty() {
                crate::catalog::cached()
                    .into_iter()
                    .flat_map(|(name, listing)| {
                        listing
                            .models
                            .into_iter()
                            .map(move |model| (name.clone(), model))
                    })
                    .collect()
            } else {
                facts
                    .iter()
                    .map(|(seat, model)| (seat.endpoint.clone(), model.clone()))
                    .collect()
            };
            held.sort_by_key(|(name, _)| name != endpoint);
            held.into_iter()
                .map(|(name, model)| {
                    let about = [name.clone(), model.window()].join(" · ");
                    let value = if name == endpoint {
                        model.id
                    } else {
                        format!("{name}/{}", model.id)
                    };
                    (value, about.trim_end_matches(" · ").to_string())
                })
                .collect()
        }
        Some(SlashCommand::Login) => plain(crate::endpoint::names().unwrap_or_default()),
        Some(SlashCommand::Effort) => plain(
            levels
                .iter()
                .map(|level| level.as_str().to_string())
                .collect(),
        ),
        Some(_) | None => Vec::new(),
    }
}

pub(super) fn mention(draft: &str, caret: usize) -> Option<(usize, &str)> {
    let before = draft.get(..caret)?;
    let start = before.rfind('@')?;
    if start > 0 && !before[..start].ends_with(char::is_whitespace) {
        return None;
    }
    let query = &before[start + 1..];
    if let Some(query) = query.strip_prefix('"') {
        (!query.contains(['"', '\n', '\r'])).then_some((start, query))
    } else {
        (!query.contains(char::is_whitespace)).then_some((start, query))
    }
}

pub(super) fn peers(text: &str, model: &Model) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for word in text
        .split_whitespace()
        .filter_map(|word| word.strip_prefix('@'))
    {
        let word = word.trim_end_matches([',', '.', ':', ';', '!', '?']);
        if model.peers.iter().any(|agent| agent.answers_to(word))
            && !names.iter().any(|name| name == word)
        {
            names.push(word.to_string());
        }
    }
    names
}

pub(super) fn reference(path: &str) -> String {
    format!("@{}", serde_json::Value::String(path.to_string()))
}

pub(super) mod widget;
pub(super) use widget::widget;
