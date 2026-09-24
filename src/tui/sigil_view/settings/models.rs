use std::collections::BTreeMap;

use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, ListRowSlots, NodeBuilder, NodeEventsExt, WidgetLayoutExt,
    container, text_field, virtual_list,
};

use super::prelude::*;
use super::{App, canvas};
use crate::catalog::{Cost, Model};
use crate::endpoint::State;
use crate::recent::Seat;

enum ModelRow {
    Header(String),
    Seat(usize),
}

fn list_rows(
    choices: &[Seat],
    counts: [usize; 3],
    groups: &BTreeMap<String, State>,
) -> Vec<ModelRow> {
    let mut rows = Vec::with_capacity(choices.len() + 8);
    let mut start = 0;
    for (title, count) in ["current", "pinned", "recent"].into_iter().zip(counts) {
        let end = (start + count).min(choices.len());
        if end > start {
            rows.push(ModelRow::Header(title.to_string()));
            rows.extend((start..end).map(ModelRow::Seat));
        }
        start = end;
    }
    let mut heading = "";
    for (index, choice) in choices.iter().enumerate().skip(start) {
        let name = choice.endpoint.as_str();
        if name != heading {
            heading = name;
            rows.push(ModelRow::Header(match groups.get(name) {
                Some(state) => format!("{name} · {state}"),
                None => name.to_string(),
            }));
        }
        rows.push(ModelRow::Seat(index));
    }
    rows
}

fn fix(name: &str, state: State, cause: Option<&String>) -> Option<String> {
    let cause = cause.and_then(|cause| cause.lines().next());
    Some(match state {
        State::NeedsKey => format!("↵ add a key for {name}"),
        State::NeedsModel => format!("{name} needs a model in config.toml"),
        State::SignIn => format!("↵ sign in to {name}"),
        State::KeyRejected => format!("{name} rejected the key. ↵ replaces the key"),
        State::RateLimited => format!("{name} is rate limited. Models still usable; F5 retries"),
        State::Offline => format!("{name} did not answer. F5 retries"),
        State::Error => format!("{name}: {}", cause.unwrap_or(state.word())),
        State::Ready | State::NoKey | State::Checking | State::Loading => return None,
    })
}

fn columns(fact: Option<&Model>, owner: &str, wide: bool, room: usize) -> ListRowSlots {
    let context = fact.map_or_else(String::new, |fact| match fact.window() {
        window if window.is_empty() => "—".to_string(),
        window => window,
    });
    let cost = match fact.map_or(Cost::Unknown, |fact| fact.cost) {
        Cost::Free => "free",
        Cost::Plan => "plan",
        Cost::Metered => "metr",
        Cost::Unknown => "",
    };
    let tag = |on: bool, tag: &'static str| if on { tag } else { "" };
    let mut columns = vec![(context, 5), (cost.to_string(), 4)];
    if let Some(fact) = fact {
        columns.push((tag(fact.fresh(), "new").to_string(), 3));
        if wide {
            columns.push((tag(fact.reasoning, "think").to_string(), 5));
            columns.push((tag(fact.vision, "img").to_string(), 3));
        }
    }
    columns.push((owner.to_string(), room));
    ListRowSlots {
        columns: columns
            .into_iter()
            .map(|(text, cells)| (text.into(), cells as f32 * canvas::cell().w))
            .collect(),
        ..ListRowSlots::default()
    }
}

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    let typing = app.clone();
    let column = || {
        container().layout(
            LayoutStyle::flex_column()
                .fill_width()
                .gap(canvas::overlay_gap()),
        )
    };
    shell(
        app.editing_model,
        "Models",
        app.viewport.peek().0,
        column()
            .child(
                container().child(
                    text_field(app.model_draft)
                        .placeholder("Search models")
                        .focus_when(app.models_focus)
                        .change(move |draft| typing.replace_model_draft(draft.to_string()))
                        .submit(|_| {})
                        .height(canvas::overlay_field()),
                ),
            )
            .child(column().children_reactive(move || body(&paint))),
    )
}

fn body(paint: &App) -> Vec<NodeBuilder> {
    let sections = paint.model_draft.get().is_empty();
    let choices = paint.model_shown.get();
    let all = paint.model_choices.get();
    let counts = paint.model_recent.get();
    let groups = paint.model_groups.get();
    let causes = paint.model_causes.get();
    let facts = paint.model_facts.get();
    let (cols, term_rows) = paint.viewport.get();
    let span = (canvas::dialog_width(cols, 72.0) / canvas::cell().w) as usize;
    let rows = if sections {
        list_rows(&choices, counts, &groups)
    } else {
        (0..choices.len()).map(ModelRow::Seat).collect()
    };
    let checking = groups
        .values()
        .filter(|state| matches!(state, State::Checking | State::Loading))
        .count();
    let mut items = Vec::new();
    let error = paint.model_error.get();
    if !error.is_empty() {
        items.push(band(
            canvas::ellipsize(&error, span.saturating_sub(10)),
            true,
        ));
    }
    let note = paint.model_note.get();
    let used = 17 + items.len() as u16;
    let visible = canvas::overlay_room(rows.len(), term_rows, used);
    let selected = paint.model_sel.get().min(choices.len().saturating_sub(1));
    if choices.is_empty() && paint.model_loading.get() {
        items.push(group("loading models…"));
    } else if choices.is_empty() {
        items.push(band(
            format!(
                "No match. ↵ uses \"{}\" as the model id.",
                paint.model_draft.peek().trim()
            ),
            false,
        ));
    } else {
        let top = rows
            .iter()
            .position(|row| matches!(row, ModelRow::Seat(index) if *index == selected))
            .map_or(0, |at| (at + 1).saturating_sub(visible));
        let wide = span >= 66;
        let owner_room = span.saturating_sub(64).clamp(8, 16);
        let source = paint.clone();
        let default = paint.model_default.get();
        let current = all.first().filter(|_| counts[0] > 0).cloned();
        let pins: Vec<Seat> = all
            .iter()
            .skip(counts[0])
            .take(counts[1])
            .cloned()
            .collect();
        let listed: usize = counts.iter().sum();
        let (shown, known, states) = (choices.clone(), facts.clone(), groups.clone());
        let line = move |index: usize| {
            let choice = &shown[index];
            let mark = if current.as_ref() == Some(choice) {
                "●"
            } else if default.as_ref() == Some(choice) {
                "◆"
            } else if pins.contains(choice) {
                "★"
            } else {
                " "
            };
            let state = states.get(&choice.endpoint);
            let owner = if sections && index >= listed {
                ""
            } else {
                choice.endpoint.as_str()
            };
            let fact = known.get(choice);
            let name = fact.map_or(choice.model.as_str(), Model::label);
            let word = state
                .filter(|state| !sections && (state.setup() || state.failed()))
                .map(|state| state.word());
            let slots = ListRowSlots {
                status: word.map(Into::into),
                ..columns(fact, owner, wide, owner_room)
            };
            (mark, name.to_string(), slots)
        };
        items.push(
            container().layout(LayoutStyle::block().fill_width()).child(
                virtual_list(rows, move |_, row: &ModelRow| match row {
                    ModelRow::Header(name) => group(name.clone()),
                    ModelRow::Seat(index) => {
                        let (mark, name, slots) = line(*index);
                        let choose = source.clone();
                        let index = *index;
                        slotted(index == selected, Some(mark), name, slots).pointer_down(
                            move |event| {
                                choose.model_sel.set(index);
                                choose.commit_model_edit();
                                event.stop_propagation();
                            },
                        )
                    }
                })
                .row_height(canvas::overlay_list_row())
                .height(canvas::overlay_list_row() * visible as f32)
                .viewport_height(canvas::overlay_list_row() * visible as f32)
                .scroll_into_view(top),
            ),
        );
    }
    let detail = choices
        .get(selected)
        .filter(|_| note.is_empty())
        .map_or(note, |choice| {
            let name = choice.endpoint.as_str();
            let fact = facts.get(choice);
            groups
                .get(name)
                .and_then(|state| fix(name, *state, causes.get(name)))
                .or_else(|| fact.and_then(|fact| fact.about.clone()))
                .unwrap_or_else(|| {
                    let label = fact.map_or_else(
                        || format!("{name}/{}", choice.model),
                        |fact| fact.label().to_string(),
                    );
                    let context = fact.map(Model::window).unwrap_or_default();
                    let cost = fact.map_or("", |fact| fact.cost.label());
                    [label.as_str(), &context, cost]
                        .into_iter()
                        .filter(|part| !part.is_empty())
                        .collect::<Vec<_>>()
                        .join(" · ")
                })
        });
    items.push(group(canvas::ellipsize(&detail, span.saturating_sub(10))));
    items.push(hint(
        if paint.model_for.peek().is_some() {
            "↵ use · alt+p pin · F5 refresh"
        } else {
            "↵ use · alt+p pin · alt+d default · F5 refresh"
        },
        match (paint.model_loading.get() && checking > 0, choices.len()) {
            (true, _) => format!("checking {checking}…"),
            (false, 0) => "0 / 0".to_string(),
            (false, total) => format!("{} / {total}", selected + 1),
        },
    ));
    items
}
