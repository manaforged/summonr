use sigil::prelude::Paint;
use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, ListRowSlots, MouseButton, NodeBuilder, NodeEventsExt,
    container, list_row_with,
};

use super::panes::ledger::{LedgerRow, Mark};
use super::{App, Model, Pane, canvas};
use crate::tui::sigil_view::ui::pick;

#[derive(Clone, PartialEq)]
struct TodosView {
    expanded: bool,
    done: usize,
    total: usize,
    detail: String,
    rows: Vec<(bool, String)>,
}

fn view(app: &App, cols: u16) -> Option<TodosView> {
    let height = app.viewport.get().1;
    let draft = app.draft.get();
    let caret = app.caret.get();
    let model = app.model.get();
    let layout = app.layout.get();
    if cols == 0
        || (layout.secondary == Some(Pane::Ledger) && layout.split(cols).1 > 0)
        || model.ledger.iter().all(|row| row.mark.settled())
        || app.commands.open(&draft, caret, &model)
    {
        return None;
    }
    let expanded = model.todos_expanded;
    let (done, total) = tally(&model.ledger);
    let current = model
        .ledger
        .iter()
        .find(|row| row.mark == Mark::Active)
        .or_else(|| model.ledger.iter().find(|row| row.mark != Mark::Done));
    let detail = current.map_or(String::new(), row_text);
    let rows = if expanded {
        let count = visible_rows(&model, height);
        let start = model
            .todos_offset
            .min(model.ledger.len().saturating_sub(count));
        model
            .ledger
            .iter()
            .skip(start)
            .take(count)
            .map(|row| (row.mark == Mark::Active, row_text(row)))
            .collect()
    } else {
        Vec::new()
    };
    Some(TodosView {
        expanded,
        done,
        total,
        detail,
        rows,
    })
}

pub(in crate::tui::sigil_view) fn height(app: &App, cols: u16) -> u16 {
    view(app, cols).map_or(0, |view| view.rows.len() as u16 + 2)
}

pub(super) fn widget(app: App, cols: u16) -> NodeBuilder {
    let update = app.clone();
    let shown = sigil::reactive::derived(move || view(&update, cols));
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0),
        )
        .children_reactive(move || {
            shown
                .get()
                .map(|view| vec![panel(app.clone(), view)])
                .unwrap_or_default()
        })
}

fn panel(app: App, view: TodosView) -> NodeBuilder {
    let toggle = app.clone();
    let mut body = container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .gap(canvas::overlay_gap())
                .padding_top(canvas::cell().h)
                .padding_x(canvas::GUTTER - 8.0),
        )
        .rounded(sigil::prelude::Radius::Base)
        .child(
            list_row_with(
                false,
                format!("Todos {}/{}", view.done, view.total),
                ListRowSlots {
                    status: view.expanded.then(|| "hide".to_string().into()),
                    detail: (!view.expanded).then(|| view.detail.clone().into()),
                    shrink: true,
                    ..ListRowSlots::default()
                },
            )
            .pointer_down(move |event| {
                if event.button == Some(MouseButton::Left) {
                    toggle.toggle_todos();
                    event.stop_propagation();
                }
            }),
        );
    for (current, text) in view.rows {
        body = body.child(pick(current, text, String::new()));
    }
    let scroll = app.clone();
    body.on_scroll(move |event| {
        let delta = canvas::scroll_lines(event.delta);
        scroll.scroll_todos(-delta.round() as isize);
        event.stop_propagation();
    })
}

pub(in crate::tui::sigil_view) fn tally(ledger: &[LedgerRow]) -> (usize, usize) {
    ledger
        .iter()
        .filter(|row| row.mark != Mark::Plain)
        .fold((0, 0), |(done, total), row| {
            (done + usize::from(row.mark == Mark::Done), total + 1)
        })
}

fn row_text(row: &LedgerRow) -> String {
    format!("{} {}", row.mark.glyph(), row.text)
}

fn visible_rows(model: &Model, rows: u16) -> usize {
    model
        .ledger
        .len()
        .min(usize::from(rows / 3).saturating_sub(2).clamp(1, 6))
}

impl App {
    pub(in crate::tui::sigil_view) fn toggle_todos(&self) {
        if self
            .model
            .peek()
            .ledger
            .iter()
            .all(|row| row.mark.settled())
        {
            self.toggle_pane(Pane::Ledger);
            return;
        }
        self.model_mut(|model| {
            model.todos_expanded = !model.todos_expanded;
            model.todos_offset = model
                .ledger
                .iter()
                .position(|row| row.mark == Mark::Active)
                .unwrap_or(0)
                .saturating_sub(1);
        });
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn scroll_todos(&self, delta: isize) -> bool {
        let rows = self.viewport.peek().1;
        let mut changed = false;
        self.model_mut(|model| {
            let count = if model.todos_expanded {
                visible_rows(model, rows)
            } else {
                1
            };
            let before = model.todos_offset;
            model.todos_offset = model
                .todos_offset
                .saturating_add_signed(delta)
                .min(model.ledger.len().saturating_sub(count));
            changed = model.todos_offset != before;
        });
        if changed {
            self.refresh();
        }
        changed
    }
}
