use sigil::prelude::{AlignItems, LayoutStyle, NodeBuilder, container, signal, virtual_list};

use super::{App, canvas};
use super::{Row, Setting, setting_rows};
use crate::tui::sigil_view::ui::{
    Bindings, Dialog, Hints, Key, Size, group, overlay_list, pick, press,
};
use sigil::prelude::Widget;

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
            let viewport = paint.viewport.get();
            let width = canvas::window_frame(viewport).0;
            if canvas::is_window() && column_count(width) > 1 {
                return vec![columns(&paint, &rows, &items, selected, width)];
            }
            let visible = canvas::overlay_shown(items.len(), viewport);
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
        .build(app.settings_open, app.viewport.peek(), &hotkeys)
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

const COLUMN_MIN: f32 = 420.0;
const COLUMN_GAP: f32 = 28.0;

fn column_count(width: f32) -> usize {
    (width / COLUMN_MIN).floor().max(1.0) as usize
}

fn sections(items: &[Item]) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    for (index, item) in items.iter().enumerate().skip(1) {
        if matches!(item, Item::Heading(_)) {
            out.push(start..index);
            start = index;
        }
    }
    if start < items.len() {
        out.push(start..items.len());
    }
    out
}

fn spread(sections: Vec<std::ops::Range<usize>>, count: usize) -> Vec<Vec<std::ops::Range<usize>>> {
    let count = count.max(1);
    let mut columns = vec![Vec::new(); count];
    let mut weight = vec![0usize; count];
    for section in sections {
        let at = weight
            .iter()
            .enumerate()
            .min_by_key(|(_, weight)| *weight)
            .map(|(index, _)| index)
            .unwrap_or(0);
        weight[at] += section.len();
        columns[at].push(section);
    }
    columns
}

fn columns(app: &App, rows: &[Row], items: &[Item], selected: usize, width: f32) -> NodeBuilder {
    let count = column_count(width).min(sections(items).len().max(1));
    let mut line = container().layout(
        LayoutStyle::flex_row()
            .fill_width()
            .gap(COLUMN_GAP)
            .align_items(AlignItems::Start),
    );
    for column in spread(sections(items), count) {
        let mut col = container().layout(
            LayoutStyle::flex_column()
                .grow(1.0)
                .fill_width()
                .min_size(0.0, 0.0)
                .gap(canvas::overlay_gap()),
        );
        for range in column {
            for item in &items[range] {
                col = col.child(item_node(app, rows, item, selected));
            }
        }
        line = line.child(col);
    }
    line
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

#[cfg(test)]
mod tests {
    use super::{Item, column_count, sections, spread};

    #[test]
    fn a_wide_menu_gains_a_side() {
        assert_eq!(column_count(400.0), 1);
        assert_eq!(column_count(900.0), 2);
        let items = [
            Item::Heading("session"),
            Item::Model(0),
            Item::Heading("model"),
            Item::Model(1),
        ];
        let columns = spread(sections(&items), 2);
        assert_eq!(columns.len(), 2);
        assert_eq!(columns[0].len(), 1);
        assert_eq!(columns[1].len(), 1);
    }
}
