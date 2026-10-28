use std::collections::BTreeMap;

use sigil::prelude::{NodeBuilder, Token};

use super::row::{price, tone};
use super::rows::{action, fix};
use crate::catalog::{Cost, Model};
use crate::endpoint::State;
use crate::recent::ModelId;
use crate::tui::sigil_view::app::{App, Mark};
use crate::tui::sigil_view::text_style::TextStyle;
use crate::tui::sigil_view::ui::{blank, field, field_width, heading, part as text};

fn kv(key: &str, value: impl Into<String>) -> NodeBuilder {
    field(key, value, Token::OnSurface)
}

pub(super) fn pane(app: &App, selection: &ModelId, fixing: bool, width: usize) -> Vec<NodeBuilder> {
    let facts = app.picker.facts.get();
    let groups = app.picker.groups.get();
    let causes = app.picker.causes.get();
    let all = app.picker.choices.get();
    let state = groups.get(&selection.endpoint).copied();
    if fixing {
        return provider(
            selection,
            state,
            causes.get(&selection.endpoint),
            &all,
            width,
        );
    }
    let fact = facts.get(selection);
    let mark = app.mark(selection);
    let mark = text(
        mark.word(),
        match mark {
            Mark::Current => Token::Success11,
            Mark::Default => Token::Accent11,
            Mark::Favorite => Token::Warn11,
            Mark::None => Token::Gray10,
        },
    );
    let name = fact
        .map_or(selection.model.as_str(), Model::label)
        .to_string();
    let mut nodes = vec![
        heading(vec![
            (name, TextStyle::sm(Token::OnSurface).bold()),
            text("   ", Token::Gray10),
            mark,
        ]),
        heading(match state {
            Some(state) => vec![
                text(
                    format!("{} · {} · ", selection.model, selection.endpoint),
                    Token::Gray10,
                ),
                text(state.word(), tone(state)),
            ],
            None => vec![text(
                format!("{} · {}", selection.model, selection.endpoint),
                Token::Gray10,
            )],
        }),
        blank(),
    ];
    nodes.extend(facts_lines(fact, width));
    nodes.extend(routes(selection, &all, &facts, &groups));
    if let Some(note) =
        state.and_then(|state| fix(&selection.endpoint, state, causes.get(&selection.endpoint)))
    {
        nodes.push(blank());
        nodes.push(heading(vec![text(
            crate::display::ellipsize(&note, width),
            state.map_or(Token::Gray10, tone),
        )]));
    }
    nodes
}

fn facts_lines(fact: Option<&Model>, width: usize) -> Vec<NodeBuilder> {
    let Some(fact) = fact else {
        return vec![kv("listed", "not in the catalog · ↵ still tries it")];
    };
    let mut nodes = Vec::new();
    let window = fact.window();
    if !window.is_empty() {
        nodes.push(kv("context", format!("{window} tokens")));
    }
    nodes.push(kv(
        "price",
        match (fact.cost, fact.price) {
            (Cost::Plan, _) => "included in the plan".to_string(),
            (Cost::Free, _) => "free".to_string(),
            (_, Some(price)) => format!(
                "{} in · {} out · {} cached  per 1M",
                crate::provider::dollars(price.input),
                crate::provider::dollars(price.output),
                crate::provider::dollars(price.cached)
            ),
            (Cost::Metered | Cost::Unknown, None) => "not listed".to_string(),
        },
    ));
    if let Some(day) = fact.released_on() {
        let fresh = if fact.fresh() { " · new" } else { "" };
        nodes.push(kv("released", format!("{day}{fresh}")));
    }
    let can = [
        (fact.reasoning, "think"),
        (fact.vision, "vision"),
        (fact.tools != Some(false), "tools"),
    ]
    .into_iter()
    .filter_map(|(on, word)| on.then_some(word))
    .collect::<Vec<_>>()
    .join(" · ");
    if !can.is_empty() {
        nodes.push(kv("can", can));
    }
    if let Some(about) = fact.about.as_deref() {
        nodes.push(kv(
            "about",
            crate::display::ellipsize(about, field_width(width)),
        ));
    }
    nodes
}

fn routes(
    selection: &ModelId,
    all: &[ModelId],
    facts: &BTreeMap<ModelId, Model>,
    groups: &BTreeMap<String, State>,
) -> Vec<NodeBuilder> {
    let mut seen = std::collections::BTreeSet::new();
    let (live, idle): (Vec<&ModelId>, Vec<&ModelId>) = all
        .iter()
        .filter(|other| other.model == selection.model && seen.insert(other.endpoint.as_str()))
        .partition(|other| {
            other.endpoint == selection.endpoint
                || groups
                    .get(&other.endpoint)
                    .is_none_or(|state| !state.blocked())
        });
    if live.len() + idle.len() < 2 {
        return Vec::new();
    }
    let mut nodes = vec![
        blank(),
        heading(vec![text("also served by", Token::Gray10)]),
    ];
    for other in live {
        let state = groups.get(&other.endpoint).copied().unwrap_or(State::Ready);
        let here = other.endpoint == selection.endpoint;
        nodes.push(heading(vec![
            text(if here { "❯ " } else { "  " }, Token::Accent11),
            text(format!("{:<20}", other.endpoint), Token::OnSurface),
            text(format!("{:<14}", state.word()), tone(state)),
            text(price(facts.get(other)), Token::Gray11),
        ]));
    }
    if !idle.is_empty() {
        let names = idle
            .iter()
            .map(|other| other.endpoint.as_str())
            .collect::<Vec<_>>();
        nodes.push(heading(vec![text(
            format!("  not set up: {}", names.join(", ")),
            Token::Gray9,
        )]));
    }
    nodes
}

fn provider(
    selection: &ModelId,
    state: Option<State>,
    cause: Option<&String>,
    all: &[ModelId],
    width: usize,
) -> Vec<NodeBuilder> {
    let state = state.unwrap_or(State::Error);
    let count = all
        .iter()
        .filter(|other| other.endpoint == selection.endpoint)
        .count();
    let mut nodes = vec![
        heading(vec![
            (
                selection.endpoint.clone(),
                TextStyle::sm(Token::OnSurface).bold(),
            ),
            text("   ", Token::Gray10),
            text(state.word(), tone(state)),
        ]),
        blank(),
    ];
    if count > 1 {
        nodes.push(kv("models", summonr_core::text::plural(count, "model")));
    }
    if let Some(cause) = cause.and_then(|cause| cause.lines().next()) {
        nodes.push(kv(
            "reason",
            crate::display::ellipsize(cause, field_width(width)),
        ));
    }
    nodes.push(blank());
    nodes.push(heading(vec![text(action(state), Token::Accent11)]));
    nodes
}
