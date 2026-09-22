use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder, NodeEventsExt, NodeStyleExt,
    ScrollDelta, classes, container,
};

use super::panes::ledger::item;
use super::prelude::*;
use super::{App, Model, Pane, canvas};

pub(super) fn widget(app: App, cols: u16) -> NodeBuilder {
    let update = app.clone();
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .shrink(0.0)
                .min_size(0.0, 0.0),
        )
        .children_reactive(move || {
            let _panes = update.panes.get();
            let _viewport = update.viewport.get();
            let draft = update.draft.get();
            let caret = update.caret.get();
            let model = update.model.borrow();
            let width = usize::from(cols);
            if width == 0
                || model.ledger.iter().all(|line| item(line).0 == '✓')
                || update.commands.open(&draft, caret, &model)
            {
                return Vec::new();
            }
            vec![panel(update.clone(), &model, width)]
        })
}

fn panel(app: App, model: &Model, width: usize) -> NodeBuilder {
    let _ = width;
    let expanded = model.todos_expanded;
    let done = model
        .ledger
        .iter()
        .filter(|line| item(line).0 == '✓')
        .count();
    let current = model
        .ledger
        .iter()
        .map(|line| item(line))
        .find(|(mark, _)| *mark == '●')
        .or_else(|| {
            model
                .ledger
                .iter()
                .map(|line| item(line))
                .find(|(mark, _)| *mark != '✓')
        });
    let detail = current.map_or(String::new(), |(mark, text)| format!("{mark} {text}"));
    let toggle = app.clone();
    let mut body = container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .gap(canvas::overlay_gap()),
        )
        .class(classes!("rounded"))
        .child(
            pick(
                false,
                false,
                format!("Tasks {done}/{}", model.ledger.len()),
                if expanded { "hide".to_string() } else { detail },
            )
            .pointer_down(move |event| {
                if event.button == Some(MouseButton::Left) {
                    toggle.toggle_todos();
                    event.stop_propagation();
                }
            }),
        );
    if expanded {
        let count = visible_rows(model, app.viewport.peek().1);
        let start = model
            .todos_offset
            .min(model.ledger.len().saturating_sub(count));
        for task in model.ledger.iter().skip(start).take(count) {
            let (mark, text) = item(task);
            body = body.child(pick(
                mark == '●',
                mark == '●',
                format!("{mark} {text}"),
                String::new(),
            ));
        }
    }
    let scroll = app.clone();
    body.on_scroll(move |event| {
        let delta = match event.delta {
            ScrollDelta::Pixels(point) => point.y / canvas::cell().h,
            ScrollDelta::Lines(point) => point.y,
        };
        scroll.scroll_todos(-delta.round() as isize);
        event.stop_propagation();
    })
}

fn visible_rows(model: &Model, rows: u16) -> usize {
    model
        .ledger
        .len()
        .min(usize::from(rows / 3).saturating_sub(2).clamp(1, 6))
}

impl App {
    pub(in crate::tui::sigil_view) fn toggle_todos(&self) {
        let mut model = self.model.borrow_mut();
        if model.ledger.iter().all(|line| item(line).0 == '✓') {
            drop(model);
            self.toggle_pane(Pane::Ledger);
            return;
        }
        model.todos_expanded = !model.todos_expanded;
        model.todos_offset = model
            .ledger
            .iter()
            .position(|line| item(line).0 == '●')
            .unwrap_or(0)
            .saturating_sub(1);
        drop(model);
        self.refresh();
    }

    pub(in crate::tui::sigil_view) fn scroll_todos(&self, delta: isize) -> bool {
        let mut model = self.model.borrow_mut();
        let count = if model.todos_expanded {
            visible_rows(&model, self.viewport.peek().1)
        } else {
            1
        };
        let before = model.todos_offset;
        model.todos_offset = model
            .todos_offset
            .saturating_add_signed(delta)
            .min(model.ledger.len().saturating_sub(count));
        let changed = model.todos_offset != before;
        drop(model);
        if changed {
            self.refresh();
        }
        changed
    }
}
