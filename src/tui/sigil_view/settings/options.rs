use sigil::prelude::{NodeBuilder, NodeEventsExt, signal, virtual_list};

use super::{App, canvas};
use super::{Row, Setting, setting_rows};
use crate::tui::sigil_view::ui::{
    Bindings, Dialog, Hints, Key, Size, group, overlay_list, pick, press,
};

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
    let count = app.clone();
    let hotkeys = app.model.peek().prefs.hotkeys.clone();
    Dialog::new(Size::Normal, "Settings", Hints::new())
        .live_hints(move || {
            Hints::new()
                .key(Key::UpDown, "move")
                .key(Key::Enter, "change")
                .key(Key::Esc, "close")
                .count(count.settings.cursor.get(), Setting::all().count())
        })
        .child(super::dialog_body(move || {
            let rows = setting_rows(&paint);
            let selected = paint
                .settings
                .cursor
                .get()
                .min(rows.len().saturating_sub(1));
            let items = list_items(&rows);
            let visible = canvas::overlay_shown(items.len(), paint.viewport.get().1);
            let shown = items
                .iter()
                .position(|item| matches!(item, Item::Model(index) if *index == selected))
                .unwrap_or(0);
            let source = paint.clone();
            vec![overlay_list(
                virtual_list(items, move |_, item: &Item| {
                    item_node(&source, &rows, item, selected)
                })
                .scroll_bind(paint.settings.scroll)
                .follow_row(signal(shown)),
                canvas::overlay_list_row(),
                visible,
            )]
        }))
        .build(app.settings_open, app.viewport.peek().0, &hotkeys)
}

fn item_node(app: &App, rows: &[Row], item: &Item, selected: usize) -> NodeBuilder {
    match item {
        Item::Heading(name) => group(*name),
        Item::Model(index) => entry(app, &rows[*index], *index, *index == selected),
    }
}

enum Item {
    Heading(&'static str),
    Model(usize),
}

fn list_items(rows: &[Row]) -> Vec<Item> {
    let mut items = Vec::with_capacity(rows.len() + 4);
    for (index, row) in rows.iter().enumerate() {
        if index == 0 || rows[index - 1].group != row.group {
            items.push(Item::Heading(row.group));
        }
        items.push(Item::Model(index));
    }
    items
}

fn entry(app: &App, row: &Row, index: usize, active: bool) -> NodeBuilder {
    let choose = app.clone();
    pick(active, row.label.clone(), row.value.clone()).pointer_down(move |event| {
        if choose.settings.cursor.peek() == index {
            choose.overlay_adjust(1);
        } else {
            choose.settings.cursor.set(index);
        }
        event.stop_propagation();
    })
}
