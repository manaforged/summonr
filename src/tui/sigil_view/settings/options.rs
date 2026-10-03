use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeEventsExt, WidgetLayoutExt, container,
    virtual_list,
};

use super::prelude::*;
use super::{App, canvas};
use super::{Row, setting_rows};

pub(in crate::tui::sigil_view) fn keys() -> Bindings {
    vec![
        press(" ", |app, event| {
            event.stop_propagation();
            app.adjust_setting();
        }),
        press("+", |app, event| {
            event.stop_propagation();
            app.overlay_adjust(1);
        }),
        press("-", |app, event| {
            event.stop_propagation();
            app.overlay_adjust(-1);
        }),
    ]
}

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    shell(
        app.settings_open,
        "Settings",
        app.viewport.peek().0,
        container()
            .layout(
                LayoutStyle::flex_column()
                    .fill_width()
                    .gap(canvas::overlay_gap()),
            )
            .children_reactive(move || {
                let rows = setting_rows(&paint);
                let selected = paint.settings_sel.get().min(rows.len().saturating_sub(1));
                let items = list_items(&rows);
                let top = items
                    .iter()
                    .position(|item| matches!(item, Item::Seat(index) if *index == selected))
                    .map_or(0, |at| at.saturating_sub(1));
                let visible = canvas::overlay_shown(items.len(), paint.viewport.get().1);
                let count = format!("{} / {}", selected + 1, rows.len());
                let lookup = rows.clone();
                let source = paint.clone();
                vec![
                    container().layout(LayoutStyle::block().fill_width()).child(
                        virtual_list(items, move |_, item: &Item| match item {
                            Item::Heading(name) => group(*name),
                            Item::Seat(index) => {
                                entry(source.clone(), &lookup[*index], *index, *index == selected)
                            }
                        })
                        .row_height(canvas::overlay_list_row())
                        .height(canvas::overlay_list_row() * visible as f32)
                        .viewport_height(canvas::overlay_list_row() * visible as f32)
                        .scroll_into_view(top),
                    ),
                    hint("↑↓ move · ↵ change · esc close", count),
                ]
            }),
    )
}

enum Item {
    Heading(&'static str),
    Seat(usize),
}

fn list_items(rows: &[Row]) -> Vec<Item> {
    let mut items = Vec::with_capacity(rows.len() + 4);
    for (index, row) in rows.iter().enumerate() {
        if index == 0 || rows[index - 1].group != row.group {
            items.push(Item::Heading(row.group));
        }
        items.push(Item::Seat(index));
    }
    items
}

fn entry(app: App, row: &Row, index: usize, active: bool) -> NodeBuilder {
    let choose = app.clone();
    pick(active, row.label.clone(), row.value.clone()).pointer_down(move |event| {
        if choose.settings_sel.peek() == index {
            choose.adjust_setting();
        } else {
            choose.settings_sel.set(index);
        }
        event.stop_propagation();
    })
}
