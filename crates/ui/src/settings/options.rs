use sigil::prelude::{NodeBuilder, Widget, signal, virtual_list};

use super::{App, Group, Row, Setting, canvas, setting_rows};
use crate::ui::{Bindings, Dialog, Hints, Key, Size, group, overlay_list, pick, press};

pub(crate) fn keys() -> Bindings {
    vec![press(" ", |app, event| {
        event.stop_propagation();
        app.adjust_setting();
    })]
}

pub(crate) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    let count = app.clone();
    let hotkeys = app.model.peek().prefs.hotkeys.clone();
    Dialog::new(Size::Normal, "Settings", Hints::new())
        .live_hints(move || {
            let category = count.settings_group.get();
            let opens = count.selected_setting().is_some_and(Setting::opens);
            Hints::new()
                .key(Key::UpDown, "move")
                .key(
                    Key::Enter,
                    if category.is_none() || opens {
                        "open"
                    } else {
                        "change"
                    },
                )
                .key(Key::Esc, if category.is_some() { "back" } else { "close" })
                .count(
                    count.settings.cursor.get(),
                    category.map_or(Group::ALL.len(), |_| setting_rows(&count).len()),
                )
        })
        .child(super::dialog_body(move || {
            let category = paint.settings_group.get();
            let rows = category.map_or_else(
                || {
                    Group::ALL
                        .into_iter()
                        .map(|group| Row {
                            label: group.name().to_string(),
                            value: "›".to_string(),
                        })
                        .collect()
                },
                |_| {
                    Setting::all()
                        .filter(|setting| Some(setting.group()) == category)
                        .zip(setting_rows(&paint))
                        .map(|(setting, mut row)| {
                            if setting.opens() {
                                row.value.push_str(" ›");
                            }
                            row
                        })
                        .collect::<Vec<_>>()
                },
            );
            let selected = paint
                .settings
                .cursor
                .get()
                .min(rows.len().saturating_sub(1));
            let visible = canvas::overlay_shown(rows.len(), paint.viewport.get());
            let source = paint.clone();
            let list = overlay_list(
                virtual_list(rows, move |index, row: &Row| {
                    entry(&source, row, index, index == selected)
                })
                .scroll_bind(paint.settings.scroll)
                .follow_row(signal(selected)),
                canvas::overlay_list_row(),
                visible,
            );
            match category {
                Some(category) => vec![group(category.name()), list],
                None => vec![list],
            }
        }))
        .build(app.settings_open, app.viewport.peek(), &hotkeys)
}

fn entry(app: &App, row: &Row, index: usize, active: bool) -> NodeBuilder {
    let choose = app.clone();
    pick(active, row.label.clone(), row.value.clone()).pointer_down(move |event| {
        choose.settings.select(index);
        choose.adjust_setting();
        event.stop_propagation();
    })
}
