use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeEventsExt, Tone, WidgetLayoutExt, container,
    virtual_list,
};

use super::{App, canvas};
use crate::prefs::Hotkey;
use crate::tui::sigil_view::ui::{Bindings, Dialog, Hints, Key, Size, notice, pick, press};

pub(in crate::tui::sigil_view) fn keys() -> Bindings {
    vec![press("r", |app, event| {
        if !app.hotkeys.recording.peek() {
            event.stop_propagation();
            app.reset_hotkeys();
        }
    })]
}

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    let state = app.clone();
    let hotkeys = app.model.peek().prefs.hotkeys.clone();
    Dialog::new(Size::Normal, "Hotkeys", Hints::new())
        .live_hints(move || {
            if state.hotkeys.recording.get() {
                Hints::new()
                    .key(Key::Fixed("ctrl+letter"), "bind")
                    .key(Key::Esc, "cancel")
            } else {
                Hints::new()
                    .key(Key::UpDown, "move")
                    .key(Key::Enter, "rebind")
                    .key(Key::Fixed("r"), "reset all")
                    .key(Key::Esc, "close")
            }
        })
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .gap(canvas::overlay_gap()),
                )
                .children_reactive(move || {
                    let selected = paint.hotkeys.sel.get().min(Hotkey::ALL.len() - 1);
                    let visible = canvas::overlay_shown(Hotkey::ALL.len(), paint.viewport.get().1);
                    let recording = paint.hotkeys.recording.get();
                    let note = paint.hotkeys.note.get();
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
                        items.push(notice(note, Tone::Error));
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
        .build(app.hotkeys.open, app.viewport.peek().0, &hotkeys)
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
        choose.hotkeys.recording.set(false);
        choose.hotkeys.sel.set(index);
        choose.edit_hotkey();
        event.stop_propagation();
    })
}

const fn description(action: Hotkey) -> &'static str {
    match action {
        Hotkey::Options => "open settings",
        Hotkey::Newline => "insert newline",
        Hotkey::Editor => "open $EDITOR",
        Hotkey::Files => "toggle files",
        Hotkey::Ledger => "expand or collapse tasks",
        Hotkey::Agents => "expand or collapse children",
        Hotkey::Diff => "toggle diff",
        Hotkey::Copy => "copy last reply",
        Hotkey::Select => "select transcript text",
        Hotkey::Pane => "hide/show pane",
        Hotkey::Search => "search conversation",
        Hotkey::History => "search saved prompts",
        Hotkey::Fold => "fold thinking, then tool output",
    }
}
