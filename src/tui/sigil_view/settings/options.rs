use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeEventsExt, WidgetLayoutExt, container,
    virtual_list,
};

use super::prelude::*;
use super::{App, canvas, watch};
use super::{Row, setting_rows};

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    shell(
        app.settings_open,
        "Options",
        container()
            .layout(
                LayoutStyle::flex_column()
                    .fill_width()
                    .gap(canvas::overlay_gap()),
            )
            .children_reactive(move || {
                watch(&paint);
                let rows = setting_rows(&paint);
                let selected = paint.settings_sel.get().min(rows.len().saturating_sub(1));
                let items = list_items(&rows);
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
                        .height(canvas::overlay_list_row() * 8.0)
                        .viewport_height(canvas::overlay_list_row() * 8.0)
                        .scroll_into_view(selected),
                    ),
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
    pick(active, false, row.label.clone(), row.value.clone()).pointer_down(move |event| {
        if choose.settings_sel.peek() == index {
            choose.adjust_setting();
        } else {
            choose.settings_sel.set(index);
        }
        event.stop_propagation();
    })
}
