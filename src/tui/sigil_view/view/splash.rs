use super::*;

pub(super) fn layer(app: App) -> NodeBuilder {
    let submit = app.clone();
    let changed = app.clone();
    let surface = app.clone();
    container()
        .layout(LayoutStyle::block().fill_width().fill_height())
        .child(
            text_edit(app.draft)
                .state(app.editor.clone())
                .caret_position(app.caret)
                .content_padding(12.0, 0.0)
                .placeholder("")
                .change(move |draft| {
                    changed.pane_focus.set(false);
                    changed.change(draft);
                })
                .submit(move |_| submit.submit())
                .layout(LayoutStyle::block().fill_width().fill_height()),
        )
        .child(scene::layer(surface))
}
