use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, NodeEventsExt, NodeStyleExt, Role, classes,
    container, label, modal, signal, virtual_list,
};

use super::super::app::App;
use crate::prefs::Hotkey;

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let paint = app.clone();
    container().layout(LayoutStyle::block()).child(
        modal(app.hotkeys_open).title("hotkeys").trap(true).child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .gap(6.0)
                        .fill_width()
                        .fill_height(),
                )
                .children_reactive(move || {
                    super::super::view::watch(&paint);
                    let selected = paint.hotkey_sel.get().min(Hotkey::ALL.len() - 1);
                    let recording = paint.hotkey_recording.get();
                    let note = paint.hotkey_note.get();
                    let rows: Vec<(Hotkey, String)> = {
                        let model = paint.model.borrow();
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
                            .row_height(24.0)
                            .viewport_fill()
                            .scroll_into_view(selected),
                        ),
                    );
                    items
                }),
        ),
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
    container()
        .role(Role::Button)
        .label(action.name().to_string())
        .focusable(true)
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(24.0)
                .padding_x(8.0)
                .gap(8.0),
        )
        .class(classes!("rounded-md"))
        .class_when(signal(active), "bg-gray-5")
        .pointer_down(move |event| {
            choose.hotkey_recording.set(false);
            choose.hotkey_sel.set(index);
            choose.edit_hotkey();
            event.stop_propagation();
        })
        .child(label(action.name().to_string()).class(classes!("text-sm text-gray-12")))
        .child(label(binding.clone()).class(classes!("text-sm text-gray-11")))
        .child(label(description(*action)).class(classes!("text-xs text-gray-11")))
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
