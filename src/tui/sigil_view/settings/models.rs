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

const EXTRA_ROWS: u16 = 3;
const SPLIT_COLS: u16 = 110;

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
    Dialog::new(Size::Page, "Models", Hints::new())
        .header(Some(tab_strip(&app, Tab::Models)))
        .live_hints(move || hints(&count))
        .child(
            container().child(
                text_field(app.picker.draft)
                    .placeholder("Search models, providers, or type a model id")
                    .focus_when(app.picker.focus)
                    .change(move |draft| typing.replace_model_draft(draft.to_string()))
                    .submit(|_| {})
                    .height(canvas::overlay_field()),
            ),
        )
        .child(container().children_reactive(move || vec![facets::chips(&chips)]))
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .gap(canvas::overlay_gap()),
                )
                .children_reactive(move || body(&paint)),
        )
        .build(app.picker.open, app.viewport.peek().0, &hotkeys)
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
    let mut items = Vec::new();
    let error = paint.picker.error.get();
    if !error.is_empty() {
        items.push(notice(
            crate::text::ellipsize(&error, span.saturating_sub(10)),
            Tone::Error,
        ));
    }
    let Some(seat) = view.seat().cloned() else {
        items.push(empty(paint));
        return items;
    };
    let used = canvas::DIALOG_CHROME + EXTRA_ROWS + items.len() as u16 + u16::from(!wide);
    let visible = canvas::overlay_room(usize::MAX, term_rows, used);
    let labels = names(&view.choices, &view.facts);
    let longest = labels
        .iter()
        .map(|name| sigil::app::cell_width(name))
        .max()
        .unwrap_or(0);
    let grid = Grid::new(span, longest, wide);
    let rows = view.rows();
    let fixing = rows
        .iter()
        .any(|row| matches!(row, ModelRow::Fix(at) if *at == view.selected));
    let note = paint.picker.note.get();
    let rows = list::list(paint, Rc::clone(&view), grid, labels, (rows, visible));
    if wide {
        items.push(beside(paint, grid, rows, (&seat, fixing), note, span));
    } else {
        items.extend([grid.header(), rows, strip(paint, &view, &seat, note, span)]);
    }
    items
}

fn empty(paint: &App) -> NodeBuilder {
    let draft = paint.picker.draft.peek().trim().to_string();
    if paint.picker.loading.get() {
        group("loading models…")
    } else if draft.is_empty() {
        notice(
            "no models match — tab changes the filter, /login adds a provider",
            Tone::Neutral,
        )
    } else {
        notice(
            format!("no match · ↵ uses \"{draft}\" as the model id"),
            Tone::Neutral,
        )
    }
}

fn strip(paint: &App, view: &View, seat: &Seat, note: String, span: usize) -> NodeBuilder {
    let state = view.groups.get(&seat.endpoint).copied();
    let causes = paint.picker.causes.get();
    let detail = if note.is_empty() {
        state
            .and_then(|state| fix(&seat.endpoint, state, causes.get(&seat.endpoint)))
            .unwrap_or_else(|| row::detail(seat, view.facts.get(seat), state))
    } else {
        note
    };
    group(crate::text::ellipsize(&detail, span.saturating_sub(10)))
}

fn beside(
    paint: &App,
    grid: Grid,
    rows: NodeBuilder,
    (seat, fixing): (&Seat, bool),
    note: String,
    span: usize,
) -> NodeBuilder {
    let list_cells = grid.cells();
    let pane_cells = span.saturating_sub(list_cells as usize + 6);
    let mut pane = detail::pane(paint, seat, fixing, pane_cells);
    if !note.is_empty() {
        pane.push(group(crate::text::ellipsize(&note, pane_cells)));
    }
    let column = |layout: LayoutStyle| container().layout(layout);
    column(
        LayoutStyle::flex_row()
            .fill_width()
            .gap(canvas::cell().w * 3.0),
    )
    .child(
        column(
            LayoutStyle::flex_column()
                .width(list_cells * canvas::cell().w)
                .shrink(0.0),
        )
        .child(grid.header())
        .child(rows),
    )
    .child(
        pane.into_iter().fold(
            column(
                LayoutStyle::flex_column()
                    .fill_width()
                    .gap(canvas::overlay_gap()),
            ),
            |node, child| node.child(child),
        ),
    )
}
