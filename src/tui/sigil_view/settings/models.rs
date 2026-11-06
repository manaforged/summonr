use std::rc::Rc;

use sigil::prelude::{LayoutStyle, NodeBuilder, Tone, container, text_field};

use super::{App, canvas};
use crate::endpoint::State;
use crate::recent::ModelId;
use crate::tui::sigil_view::ui::{
    Bindings, Dialog, Hints, Key, KeyAction, Size, Tab, browser_columns, browser_layout, group,
    input_keys, notice, tab_strip,
};

mod detail;
mod facets;
mod list;
mod row;
mod rows;

const LIST_CHROME: u16 = 7;
const SPLIT_COLS: u16 = 140;

use list::View;
use row::Grid;
pub(in crate::tui::sigil_view) use row::{price, tone};
use rows::{ModelRow, fix, names};
use sigil::prelude::Widget;

pub(in crate::tui::sigil_view) fn keys() -> Bindings {
    let mut bindings = vec![(
        sigil::prelude::Key::F5.into(),
        (|app, event| {
            event.stop_propagation();
            app.refresh_models();
        }) as KeyAction,
    )];
    bindings.push((
        sigil::prelude::Shortcut::ctrl(sigil::prelude::LogicalKey::character("s")),
        |app, event| {
            event.stop_propagation();
            app.pin_model();
        },
    ));
    bindings.push((sigil::prelude::Key::F2.into(), |app, event| {
        event.stop_propagation();
        if app.picker.target.peek().is_none() {
            app.default_model();
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
                input_keys(&app, text_field(app.picker.list.query))
                    .placeholder("Search models, providers, or type a model id")
                    .focus_when(app.picker.focus)
                    .autofocus()
                    .change(move |draft| typing.replace_model_draft(draft.to_string()))
                    .submit(|_| {})
                    .height(canvas::overlay_field()),
            ),
        )
        .child(
            container()
                .layout(LayoutStyle::flex_column().padding_bottom(canvas::overlay_list_row()))
                .children_reactive(move || vec![facets::chips(&chips)]),
        )
        .child(container().children_reactive(move || body(&paint)))
        .build(app.picker.open, (cols, rows), &hotkeys)
}

fn hints(app: &App) -> Hints {
    let total = app.picker.shown.get().len();
    let selected = app.picker.list.cursor.get().min(total.saturating_sub(1));
    let checking = app
        .picker
        .groups
        .get()
        .values()
        .filter(|state| matches!(state, State::Checking | State::Loading))
        .count();
    let mut hints = Hints::new()
        .key(Key::Enter, "use")
        .key(Key::Tab, "filter")
        .key(Key::Fixed("ctrl+s"), "favorite");
    if app.picker.target.get().is_none() {
        hints = hints.key(Key::Fixed("F2"), "default");
    }
    hints.key(Key::Fixed("F5"), "refresh").right(
        match (app.picker.loading.get() && checking > 0, total) {
            (true, _) => format!("checking {checking}…"),
            (false, 0) => "0 / 0".to_string(),
            (false, total) => format!("{} / {total}", selected + 1),
        },
    )
}

fn body(paint: &App) -> Vec<NodeBuilder> {
    let view = Rc::new(View::read(paint));
    let (wide, span, visible) = browser_layout(paint.viewport.get(), SPLIT_COLS, (LIST_CHROME, 1));
    let labels = names(&view.choices, &view.facts);
    let longest = labels
        .iter()
        .map(|label| sigil::app::cell_width(label))
        .max()
        .unwrap_or(0);
    let varied = view
        .choices
        .iter()
        .filter_map(|choice| view.facts.get(choice).map(row::abilities))
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        > 1;
    let grid = Grid::new(span, wide, longest, varied);
    let rows = view.rows();
    let fixing = rows
        .iter()
        .any(|row| matches!(row, ModelRow::Fix(at) if *at == view.selected));
    let selection = view.selection().cloned();
    let list = match selection {
        Some(_) => list::list(paint, Rc::clone(&view), grid, labels, (rows, visible)),
        None => empty(paint),
    };
    let height = (visible + 1) as f32 * canvas::overlay_list_row();
    let frame = LayoutStyle::flex_column().height(height).shrink(0.0);
    if !wide {
        let narrow = span.saturating_sub(10);
        let rows = if paint.auth.open.get() {
            super::auth::step_pane(paint, narrow, visible)
                .into_iter()
                .fold(container().layout(frame.fill_width()), |node, child| {
                    node.child(child)
                })
        } else {
            container()
                .layout(frame.fill_width())
                .child(grid.header())
                .child(list)
        };
        return vec![rows, strip(paint, &view, selection.as_ref(), narrow)];
    }
    let width = span.saturating_sub(grid.cells() as usize + 6);
    browser_columns(
        [grid.header(), list],
        frame.width(grid.cells() * canvas::cell().w),
        side(paint, selection.as_ref(), fixing, (width, visible)),
        canvas::cell().w * 3.0,
    )
}

fn empty(paint: &App) -> NodeBuilder {
    let draft = paint.picker.list.query.peek().trim().to_string();
    if paint.picker.loading.get() {
        group("loading models…")
    } else if draft.is_empty() {
        group("no models match; tab changes the filter, /login adds a provider")
    } else {
        group(format!("no match · ↵ uses \"{draft}\" as the model id"))
    }
}

fn side(
    paint: &App,
    selection: Option<&ModelId>,
    fixing: bool,
    (width, visible): (usize, usize),
) -> Vec<NodeBuilder> {
    let mut nodes = Vec::new();
    let error = paint.picker.error.get();
    if !error.is_empty() {
        nodes.push(notice(
            crate::display::ellipsize(&error, width),
            Tone::Error,
        ));
    }
    let note = paint.picker.note.get();
    if !note.is_empty() {
        nodes.push(group(crate::display::ellipsize(&note, width)));
    }
    if paint.auth.open.get() {
        nodes.extend(super::auth::step_pane(paint, width, visible));
    } else if let Some(selection) = selection {
        nodes.extend(detail::pane(paint, selection, fixing, width));
    }
    nodes
}

fn strip(paint: &App, view: &View, selection: Option<&ModelId>, width: usize) -> NodeBuilder {
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
    } else if let Some(selection) = selection {
        let state = view.groups.get(&selection.endpoint).copied();
        state
            .and_then(|state| fix(&selection.endpoint, state, causes.get(&selection.endpoint)))
            .unwrap_or_else(|| row::detail(selection, view.facts.get(selection), state))
    } else {
        String::new()
    };
    group(crate::display::ellipsize(&text, width))
}
