use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeEventsExt, NodeStyleExt, WidgetLayoutExt,
    classes, container, label, virtual_list,
};

use super::prelude::*;
use super::{App, canvas};
use crate::prefs::Hotkey;

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    shell(
        app.hotkeys_open,
        "Hotkeys",
        app.viewport.peek().0,
        container()
            .layout(
                LayoutStyle::flex_column()
                    .fill_width()
                    .gap(canvas::overlay_gap()),
            )
            .children_reactive(move || {
                let selected = paint.hotkey_sel.get().min(Hotkey::ALL.len() - 1);
                let visible = canvas::overlay_shown(Hotkey::ALL.len(), paint.viewport.get().1);
                let recording = paint.hotkey_recording.get();
                let note = paint.hotkey_note.get();
                let rows: Vec<(Hotkey, String)> = {
                    let model = paint.model.get();
                    Hotkey::ALL
                        .iter()
                        .copied()
                        .map(|action| {
                            let binding = if recording && action == Hotkey::ALL[selected] {
                                "press ctrl+letter".to_string()
                            } else {
                                model.prefs.hotkeys.label(action)
                            };
                            (action, binding)
                        })
                        .collect()
                };
                let source = paint.clone();
                let mut items: Vec<NodeBuilder> = Vec::new();
                if !note.is_empty() {
                    items.push(
                        container().child(label(note).class(classes!("text-xs text-error-11"))),
                    );
                }
                items.push(
                    container().layout(LayoutStyle::block().fill_width()).child(
                        virtual_list(rows, move |index, row: &(Hotkey, String)| {
                            entry(source.clone(), index, row, selected)
                        })
                        .row_height(canvas::overlay_list_row())
                        .height(canvas::overlay_list_row() * visible as f32)
                        .viewport_height(canvas::overlay_list_row() * visible as f32)
                        .scroll_into_view(selected),
                    ),
                );
                items
            }),
    )
}

fn entry(
    app: App,
    index: usize,
    (action, binding): &(Hotkey, String),
    selected: usize,
) -> NodeBuilder {
    let choose = app.clone();
    let active = index == selected;
    pick(
        active,
        action.name().to_string(),
        format!("{} · {}", binding, description(*action)),
    )
    .pointer_down(move |event| {
        choose.hotkey_recording.set(false);
        choose.hotkey_sel.set(index);
        choose.edit_hotkey();
        event.stop_propagation();
    })
}

const fn description(action: Hotkey) -> &'static str {
    match action {
        Hotkey::Options => "open options",
        Hotkey::Newline => "insert newline",
        Hotkey::Editor => "open $EDITOR",
        Hotkey::Files => "toggle files",
        Hotkey::Ledger => "expand or collapse tasks",
        Hotkey::Diff => "toggle diff",
        Hotkey::Copy => "copy last reply",
        Hotkey::Select => "select transcript text",
        Hotkey::Pane => "hide/show pane",
        Hotkey::Search => "search conversation",
        Hotkey::History => "search saved prompts",
    }
}
