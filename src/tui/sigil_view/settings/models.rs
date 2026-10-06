use std::rc::Rc;

use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, Tone, WidgetLayoutExt, container, text_field,
};

use super::{App, canvas};
use crate::endpoint::State;
use crate::recent::Seat;
use crate::tui::sigil_view::ui::{
    Bindings, Dialog, Hints, Key, KeyAction, Size, Tab, alt, group, notice, tab_strip,
};

mod detail;
mod facets;
mod list;
mod row;
mod rows;

const LIST_CHROME: u16 = 6;
const SPLIT_COLS: u16 = 140;

use list::View;
use row::Grid;
pub(in crate::tui::sigil_view::settings) use row::{price, tone};
use rows::{ModelRow, fix, names};

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
    let count = app.clone();
    let chips = app.clone();
    let hotkeys = app.model.peek().prefs.hotkeys.clone();
    let (cols, rows) = app.viewport.peek();
    Dialog::new(Size::Page, "Models", Hints::new())
        .header(Some(tab_strip(&app, Tab::Models)))
        .fixed(rows)
        .live_hints(move || {
            if count.auth.open.get() {
                super::auth::hints(&count)
            } else {
                hints(&count)
            }
        })
        .child(
            container().child(
                text_field(app.picker.draft)
                    .placeholder("Search models, providers, or type a model id")
                    .focus_when(app.picker.focus)
                    .autofocus()
                    .change(move |draft| typing.replace_model_draft(draft.to_string()))
                    .submit(|_| {})
                    .height(canvas::overlay_field()),
            ),
        )
        .child(container().children_reactive(move || vec![facets::chips(&chips)]))
        .child(container().children_reactive(move || body(&paint)))
        .build(app.picker.open, cols, &hotkeys)
}

fn hints(app: &App) -> Hints {
    let total = app.picker.shown.get().len();
    let selected = app.picker.sel.get().min(total.saturating_sub(1));
    let checking = app
        .picker
        .groups
        .get()
        .values()
        .filter(|state| matches!(state, State::Checking | State::Loading))
        .count();
    let mut hints = Hints::new()
        .key(Key::Enter, "use")
        .key(Key::Fixed("alt+p"), "favorite");
    if app.picker.target.get().is_none() {
        hints = hints.key(Key::Fixed("alt+d"), "default");
    }
    hints
        .key(Key::Fixed("alt+o"), "older")
        .key(Key::Fixed("F5"), "refresh")
        .right(match (app.picker.loading.get() && checking > 0, total) {
            (true, _) => format!("checking {checking}…"),
            (false, 0) => "0 / 0".to_string(),
            (false, total) => format!("{} / {total}", selected + 1),
        })
}

fn split(cols: u16) -> bool {
    cols >= SPLIT_COLS && !canvas::is_window()
}

fn body(paint: &App) -> Vec<NodeBuilder> {
    let view = Rc::new(View::read(paint));
    let (cols, term_rows) = paint.viewport.get();
    let span = (canvas::dialog_width(cols, f32::from(cols)) / canvas::cell().w) as usize;
    let wide = split(cols);
    let visible = canvas::list_rows(term_rows, LIST_CHROME + u16::from(!wide));
    let grid = Grid::new(span, wide);
    let rows = view.rows();
    let fixing = rows
        .iter()
        .any(|row| matches!(row, ModelRow::Fix(at) if *at == view.selected));
    let seat = view.seat().cloned();
    let list = match seat {
        Some(_) => {
            let labels = names(&view.choices, &view.facts);
            list::list(paint, Rc::clone(&view), grid, labels, (rows, visible))
        }
        None => empty(paint),
    };
    let height = (visible + 1) as f32 * canvas::cell().h;
    let column = |layout: LayoutStyle| container().layout(layout);
    let frame = LayoutStyle::flex_column().height(height).shrink(0.0);
    if !wide {
        let narrow = span.saturating_sub(10);
        let rows = if paint.auth.open.get() {
            super::auth::step_pane(paint, narrow, visible)
                .into_iter()
                .fold(column(frame.fill_width()), |node, child| node.child(child))
        } else {
            column(frame.fill_width()).child(grid.header()).child(list)
        };
        return vec![rows, strip(paint, &view, seat.as_ref(), narrow)];
    }
    let rows = column(frame.width(grid.cells() * canvas::cell().w))
        .child(grid.header())
        .child(list);
    let width = span.saturating_sub(grid.cells() as usize + 6);
    let pane = side(paint, seat.as_ref(), fixing, (width, visible));
    vec![
        column(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(canvas::cell().w * 3.0),
        )
        .child(rows)
        .child(
            pane.into_iter().fold(
                column(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .gap(canvas::overlay_gap()),
                ),
                |node, child| node.child(child),
            ),
        ),
    ]
}

fn empty(paint: &App) -> NodeBuilder {
    let draft = paint.picker.draft.peek().trim().to_string();
    if paint.picker.loading.get() {
        group("loading models…")
    } else if draft.is_empty() {
        group("no models match — tab changes the filter, /login adds a provider")
    } else {
        group(format!("no match · ↵ uses \"{draft}\" as the model id"))
    }
}

fn side(
    paint: &App,
    seat: Option<&Seat>,
    fixing: bool,
    (width, visible): (usize, usize),
) -> Vec<NodeBuilder> {
    let mut nodes = Vec::new();
    let error = paint.picker.error.get();
    if !error.is_empty() {
        nodes.push(notice(crate::text::ellipsize(&error, width), Tone::Error));
    }
    let note = paint.picker.note.get();
    if !note.is_empty() {
        nodes.push(group(crate::text::ellipsize(&note, width)));
    }
    if paint.auth.open.get() {
        nodes.extend(super::auth::step_pane(paint, width, visible));
    } else if let Some(seat) = seat {
        nodes.extend(detail::pane(paint, seat, fixing, width));
    }
    nodes
}

fn strip(paint: &App, view: &View, seat: Option<&Seat>, width: usize) -> NodeBuilder {
    if paint.auth.open.get() {
        return group("");
    }
    let error = paint.picker.error.get();
    let note = paint.picker.note.get();
    let causes = paint.picker.causes.get();
    let text = if !error.is_empty() {
        error
    } else if !note.is_empty() {
        note
    } else if let Some(seat) = seat {
        let state = view.groups.get(&seat.endpoint).copied();
        state
            .and_then(|state| fix(&seat.endpoint, state, causes.get(&seat.endpoint)))
            .unwrap_or_else(|| row::detail(seat, view.facts.get(seat), state))
    } else {
        String::new()
    };
    group(crate::text::ellipsize(&text, width))
}
