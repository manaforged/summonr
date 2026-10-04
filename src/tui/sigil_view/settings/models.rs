use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeEventsExt, WidgetLayoutExt, container,
    text_field, virtual_list,
};

use super::prelude::*;
use super::{App, canvas};
use crate::catalog::Model;
use crate::endpoint::State;
use crate::recent::Seat;

mod row;

const EXTRA_ROWS: u16 = 2;

use row::{Grid, Shown};

enum ModelRow {
    Header(String),
    Provider(String, usize),
    Seat(usize),
    Older(usize),
}

fn list_rows(
    choices: &[Seat],
    counts: [usize; 3],
    groups: &std::collections::BTreeMap<String, State>,
    aged: usize,
) -> Vec<ModelRow> {
    let mut rows = Vec::with_capacity(choices.len() + 8);
    let mut unavailable = Vec::new();
    let mut start = 0;
    for (title, count) in ["current", "favorites", "recent"].into_iter().zip(counts) {
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
        if groups.get(name).copied().is_some_and(row::blocked) {
            unavailable.push(ModelRow::Seat(index));
            continue;
        }
        if name != heading {
            heading = name;
            let count = choices[index..]
                .iter()
                .take_while(|seat| seat.endpoint == name)
                .count();
            rows.push(ModelRow::Provider(name.to_string(), count));
        }
        rows.push(ModelRow::Seat(index));
    }
    if aged > 0 {
        rows.push(ModelRow::Older(aged));
    }
    if !unavailable.is_empty() {
        rows.push(ModelRow::Header("unavailable".to_string()));
        rows.extend(unavailable);
    }
    rows
}

fn label<'a>(seat: &'a Seat, facts: &'a std::collections::BTreeMap<Seat, Model>) -> &'a str {
    facts.get(seat).map_or(seat.model.as_str(), Model::label)
}

fn names(choices: &[Seat], facts: &std::collections::BTreeMap<Seat, Model>) -> Vec<String> {
    let mut seen: std::collections::BTreeMap<(&str, &str), usize> =
        std::collections::BTreeMap::new();
    for seat in choices {
        *seen
            .entry((seat.endpoint.as_str(), label(seat, facts)))
            .or_default() += 1;
    }
    choices
        .iter()
        .map(|seat| {
            let name = label(seat, facts);
            if seen
                .get(&(seat.endpoint.as_str(), name))
                .copied()
                .unwrap_or(0)
                > 1
            {
                seat.model.clone()
            } else {
                name.to_string()
            }
        })
        .collect()
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

pub(in crate::tui::sigil_view) fn keys() -> Bindings {
    let mut bindings = vec![(
        sigil::prelude::Key::F5.into(),
        (|app, event| {
            event.stop_propagation();
            app.refresh_models();
        }) as KeyAction,
    )];
    bindings.extend(alt("p", |app, event| {
        event.stop_propagation();
        app.pin_model();
    }));
    bindings.extend(alt("d", |app, event| {
        event.stop_propagation();
        if app.picker.target.peek().is_none() {
            app.default_model();
        }
    }));
    bindings.extend(alt("o", |app, event| {
        if app.picker.draft.peek().is_empty() {
            event.stop_propagation();
            app.toggle_older_models();
        }
    }));
    bindings
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
    wide_shell(
        app.picker.open,
        "Models",
        app.viewport.peek().0,
        column()
            .child(
                container().child(
                    text_field(app.picker.draft)
                        .placeholder("Search models")
                        .focus_when(app.picker.focus)
                        .change(move |draft| typing.replace_model_draft(draft.to_string()))
                        .submit(|_| {})
                        .height(canvas::overlay_field()),
                ),
            )
            .child(column().children_reactive(move || body(&paint))),
    )
}

fn body(paint: &App) -> Vec<NodeBuilder> {
    let sections = paint.picker.draft.get().is_empty();
    let choices = paint.picker.shown.get();
    let all = paint.picker.choices.get();
    let counts = paint.picker.recent.get();
    let groups = paint.picker.groups.get();
    let causes = paint.picker.causes.get();
    let facts = paint.picker.facts.get();
    let (cols, term_rows) = paint.viewport.get();
    let span = (canvas::dialog_width(cols, canvas::WIDE_DIALOG_CELLS) / canvas::cell().w) as usize;
    let listed: usize = counts.iter().sum();
    let older = paint.picker.older.get();
    let aged = if sections {
        paint.picker.hidden.get()
    } else {
        0
    };
    let rows = if sections {
        list_rows(&choices, counts, &groups, aged)
    } else {
        (0..choices.len()).map(ModelRow::Seat).collect()
    };
    let checking = groups
        .values()
        .filter(|state| matches!(state, State::Checking | State::Loading))
        .count();
    let mut items = Vec::new();
    let error = paint.picker.error.get();
    if !error.is_empty() {
        items.push(band(
            crate::text::ellipsize(&error, span.saturating_sub(10)),
            true,
        ));
    }
    let note = paint.picker.note.get();
    let used = canvas::DIALOG_CHROME + EXTRA_ROWS + items.len() as u16;
    let visible = canvas::overlay_room(rows.len(), term_rows, used);
    let selected = paint.picker.sel.get().min(choices.len().saturating_sub(1));
    if choices.is_empty() && paint.picker.loading.get() {
        items.push(group("loading models…"));
    } else if choices.is_empty() {
        let draft = paint.picker.draft.peek().trim().to_string();
        items.push(band(
            if draft.is_empty() {
                "no models — /login adds a provider".to_string()
            } else {
                format!("no match · ↵ uses \"{draft}\" as the model id")
            },
            false,
        ));
    } else {
        let top = rows
            .iter()
            .position(|row| matches!(row, ModelRow::Seat(index) if *index == selected))
            .map_or(0, |at| (at + 1).saturating_sub(visible));
        let source = paint.clone();
        let expand = paint.clone();
        let default = paint.picker.default.get();
        let current = all.first().filter(|_| counts[0] > 0).cloned();
        let pins: Vec<Seat> = all
            .iter()
            .skip(counts[0])
            .take(counts[1])
            .cloned()
            .collect();
        let labels = names(&choices, &facts);
        let grid = Grid::new(
            span,
            labels
                .iter()
                .map(|name| sigil::app::cell_width(name))
                .max()
                .unwrap_or(0),
        );
        items.push(grid.header());
        let (shown, known, states) = (choices.clone(), facts.clone(), groups.clone());
        let headings = groups.clone();
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
            let fact = known.get(choice);
            let state = states.get(&choice.endpoint).copied();
            let slots = grid.row(
                choice,
                fact,
                Shown {
                    provider: !sections || index < listed || state.is_some_and(row::blocked),
                    state,
                },
            );
            (mark, labels[index].clone(), slots)
        };
        items.push(
            container().layout(LayoutStyle::block().fill_width()).child(
                virtual_list(rows, move |_, row: &ModelRow| match row {
                    ModelRow::Header(name) => row::section_heading(name),
                    ModelRow::Provider(name, count) => {
                        row::provider_heading(name, headings.get(name).copied(), *count)
                    }
                    ModelRow::Older(count) => {
                        let toggle = expand.clone();
                        let noun = if *count == 1 { "model" } else { "models" };
                        group(if older {
                            format!("▾ hide {count} older {noun} · alt+o")
                        } else {
                            format!("▸ show {count} older {noun} · alt+o")
                        })
                        .pointer_down(move |event| {
                            toggle.toggle_older_models();
                            event.stop_propagation();
                        })
                    }
                    ModelRow::Seat(index) => {
                        let (mark, name, slots) = line(*index);
                        let choose = source.clone();
                        let follow = source.clone();
                        let index = *index;
                        slotted(index == selected, Some(mark), name, slots)
                            .pointer_move(move |_| {
                                if follow.picker.sel.peek() != index {
                                    follow.picker.sel.set(index);
                                }
                            })
                            .pointer_down(move |event| {
                                choose.picker.sel.set(index);
                                choose.commit_model_edit();
                                event.stop_propagation();
                            })
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
            let state = groups.get(name).copied();
            state
                .and_then(|state| fix(name, state, causes.get(name)))
                .unwrap_or_else(|| row::detail(choice, facts.get(choice), state))
        });
    items.push(group(crate::text::ellipsize(
        &detail,
        span.saturating_sub(10),
    )));
    items.push(hint(
        if paint.picker.target.peek().is_some() {
            "↵ use · alt+p favorite · F5 refresh"
        } else {
            "↵ use · alt+p favorite · alt+d default · F5 refresh"
        },
        match (paint.picker.loading.get() && checking > 0, choices.len()) {
            (true, _) => format!("checking {checking}…"),
            (false, 0) => "0 / 0".to_string(),
            (false, total) => format!("{} / {total}", selected + 1),
        },
    ));
    items
}
