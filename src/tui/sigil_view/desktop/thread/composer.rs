use super::*;

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
                .shrink(0.0),
        )
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .fill_width()
                        .padding(16.0)
                        .gap(12.0),
                )
                .class(classes!("rounded-lg border border-gray-6 bg-gray-1"))
                .child(
                    text_edit(value)
                        .caret_position(caret)
                        .placeholder(if editing {
                            "editing the file — ctrl+s saves, esc leaves"
                        } else {
                            "ask for changes — @file attaches, /help lists"
                        })
                        .change(move |draft| {
                            if !editing {
                                changed.change(draft);
                            }
                        })
                        .submit(move |_| submit.submit())
                        .height(72.0)
                        .class(classes!("border-0 bg-gray-1")),
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
                            text_live(move || {
                                drop(model.header.get());
                                model.model.borrow().snapshot.model.clone()
                            })
                            .class(classes!("text-sm text-gray-12")),
                        )
                        .child(spacer())
                        .child(
                            hit("Send".to_string(), false, move || send.submit())
                                .layout(
                                    LayoutStyle::flex_row()
                                        .fixed_size(28.0, 28.0)
                                        .shrink(0.0)
                                        .align_items(AlignItems::Center)
                                        .justify_content(JustifyContent::Center),
                                )
                                .class(classes!("rounded-full bg-gray-12"))
                                .child(icons::send()),
                        ),
                ),
        )
}
