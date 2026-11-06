use sigil::prelude::{
    AlignItems, ControlSize, LayoutStyle, NodeBuilder, Paint, Rgba8, Signal, Tone, Widget, button,
    container, signal, text_edit,
};

use super::{App, canvas};
use crate::tui::sigil_view::theme::{Palette, color_hex};
use crate::tui::sigil_view::ui::{
    Bindings, Dialog, Hints, Key, List, Size, input_keys, notice, pick,
};

mod actions;

const LIST_CHROME: usize = 8;
const EDIT_CHROME: usize = 4;

#[derive(Clone, Copy)]
pub(in crate::tui::sigil_view) struct Editor {
    pub(in crate::tui::sigil_view) open: Signal<bool>,
    pub(in crate::tui::sigil_view) list: List,
    palette: Signal<Palette>,
    before: Signal<Palette>,
    name: Signal<String>,
    input: Signal<String>,
    caret: Signal<usize>,
    focus: Signal<u64>,
    editing: Signal<Option<(&'static str, Rgba8)>>,
    error: Signal<String>,
}

impl Editor {
    pub(in crate::tui::sigil_view) fn new() -> Self {
        Self {
            open: signal(false),
            list: List::new(),
            palette: signal(Palette::SPACE),
            before: signal(Palette::SPACE),
            name: signal(String::new()),
            input: signal(String::new()),
            caret: signal(0),
            focus: signal(0),
            editing: signal(None),
            error: signal(String::new()),
        }
    }
}

pub(in crate::tui::sigil_view) fn keys() -> Bindings {
    vec![(
        sigil::prelude::Shortcut::ctrl(sigil::prelude::LogicalKey::character("s")),
        |app, event| {
            event.stop_propagation();
            app.save_theme();
        },
    )]
}

pub(in crate::tui::sigil_view) fn layer(app: App) -> NodeBuilder {
    let editor = app.theme_editor;
    let paint = app.clone();
    let choose = app.clone();
    let state = app.clone();
    let viewport = app.viewport;
    let keys = app.model.peek().prefs.hotkeys.clone();
    Dialog::new(Size::Reading, "Theme editor", Hints::new())
        .live_hints(move || {
            if state.theme_editor.editing.get().is_some() {
                Hints::new()
                    .key(Key::Enter, "apply color")
                    .key(Key::Esc, "cancel color")
            } else {
                Hints::new()
                    .key(Key::UpDown, "move")
                    .key(Key::Enter, "edit")
                    .key(Key::Fixed("←/→"), "preset")
                    .key(Key::Fixed("ctrl+s"), "save")
                    .key(Key::Esc, "cancel")
            }
        })
        .child(container().children_reactive(move || {
            let save = paint.clone();
            let cancel = paint.clone();
            let reset = paint.clone();
            let mut nodes = vec![
                container()
                    .layout(LayoutStyle::flex_row().fill_width().gap(8.0))
                    .child(
                        button("Save")
                            .primary()
                            .size(ControlSize::Compact)
                            .activate(move || save.save_theme()),
                    )
                    .child(
                        button("Cancel")
                            .ghost()
                            .size(ControlSize::Compact)
                            .activate(move || cancel.close_theme()),
                    )
                    .child(
                        button("Reset preset")
                            .ghost()
                            .size(ControlSize::Compact)
                            .activate(move || reset.reset_theme()),
                    ),
            ];
            nodes.push(
                {
                    let window = crate::tui::sigil_view::canvas::is_window();
                    [
                        ("System", crate::prefs::AUTO_THEME),
                        ("Light", if window { "paper" } else { "dawn" }),
                        ("Dark", if window { "iron" } else { "space" }),
                    ]
                }
                .into_iter()
                .fold(
                    container().layout(LayoutStyle::flex_row().fill_width().gap(8.0)),
                    |row, (label, name)| {
                        let select = paint.clone();
                        row.child(
                            button(label)
                                .ghost()
                                .size(ControlSize::Compact)
                                .activate(move || select.select_theme(name)),
                        )
                    },
                ),
            );
            if let Some((name, _)) = editor.editing.get() {
                let change = paint.clone();
                let apply = paint.clone();
                nodes.push(
                    container()
                        .child(
                            sigil::prelude::text(format!("{name} · #rgb, #rrggbb, or #rrggbbaa"))
                                .nowrap()
                                .font_size(12.0),
                        )
                        .child(
                            input_keys(&paint, text_edit(editor.input))
                                .caret_position(editor.caret)
                                .focus_when(editor.focus)
                                .autofocus()
                                .layout(
                                    LayoutStyle::flex_column()
                                        .fill_width()
                                        .height(canvas::overlay_field()),
                                )
                                .change(move |value| change.preview_color(&value))
                                .submit(move |_| apply.edit_theme()),
                        ),
                );
            }
            let error = editor.error.get();
            if !error.is_empty() {
                nodes.push(notice(error, Tone::Error));
            }
            nodes
        }))
        .collection(
            editor.list,
            move || {
                let colors = editor.palette.get().colors();
                Ok(
                    std::iter::once(("Preset".to_string(), editor.name.get(), None))
                        .chain(
                            colors.into_iter().map(|(name, color)| {
                                (name.to_string(), color_hex(color), Some(color))
                            }),
                        )
                        .collect::<Vec<_>>(),
                )
            },
            String::new,
            move |len| {
                Some((
                    canvas::overlay_list_row(),
                    canvas::overlay_shown(len, viewport.get())
                        .saturating_sub(
                            LIST_CHROME + usize::from(editor.editing.get().is_some()) * EDIT_CHROME,
                        )
                        .max(1),
                ))
            },
            |(name, value, color), active| {
                let mut row = container().layout(
                    LayoutStyle::flex_row()
                        .fill_width()
                        .align_items(AlignItems::Center),
                );
                if let Some(color) = color {
                    row = row.child(
                        container()
                            .layout(
                                LayoutStyle::block()
                                    .width(canvas::cell().w * 2.0)
                                    .height(canvas::overlay_list_row()),
                            )
                            .bg(*color),
                    );
                }
                row.child(
                    container()
                        .layout(LayoutStyle::flex_column().grow(1.0).min_size(0.0, 0.0))
                        .child(pick(active, name.clone(), value.clone())),
                )
            },
            move || choose.edit_theme(),
        )
        .build(editor.open, viewport.peek(), &keys)
}
