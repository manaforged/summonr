use super::*;
use sigil::prelude::{button, card, signal, spacer};

pub(super) fn composer(app: App, editing: bool) -> NodeBuilder {
    let submit = app.clone();
    let send = app.clone();
    let changed = app.clone();
    let paste = app.clone();
    let paste_meta = app.clone();
    let (value, caret) = if editing {
        (app.view_text, app.view_caret)
    } else {
        (app.draft, app.caret)
    };
    let model = app.clone();
    let focus = signal(0_u64);
    container()
        .key_capture(Shortcut::ctrl(LogicalKey::character("v")), move |event| {
            if paste.clipboard() {
                event.stop_propagation();
            }
        })
        .key_capture(
            Shortcut::new(LogicalKey::character("v"), ModifiersState::META),
            move |event| {
                if paste_meta.clipboard() {
                    event.stop_propagation();
                }
            },
        )
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .padding_x(24.0)
                .padding_y(12.0)
                .shrink(0.0)
                .align_items(AlignItems::Stretch),
        )
        .pointer_down(move |_| focus.update(|n| *n += 1))
        .child(card(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .gap(12.0)
                        .min_size(0.0, 120.0),
                )
                .class("bg-gray-3")
                .child(
                    text_edit(value)
                        .caret_position(caret)
                        .placeholder(if editing {
                            "editing the file — ctrl+s saves, esc leaves"
                        } else {
                            "Message — @file attaches, /help lists"
                        })
                        .change(move |draft| {
                            if !editing {
                                changed.change(draft);
                            }
                        })
                        .submit(move |_| submit.submit())
                        .focus_when(focus)
                        .layout(LayoutStyle::flex_column().fill_width().height(96.0))
                        .class("text-sm text-gray-12"),
                )
                .child(
                    container()
                        .layout(
                            LayoutStyle::flex_row()
                                .fill_width()
                                .align_items(AlignItems::Center)
                                .gap(8.0),
                        )
                        .child(
                            sigil::kernel::text(move || {
                                drop(model.header.get());
                                model.model.get().snapshot.model.clone()
                            })
                            .class("text-sm text-gray-12"),
                        )
                        .child(spacer())
                        .child(button("Send").activate(move || send.submit())),
                ),
        ))
}
