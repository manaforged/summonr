use super::*;
use crate::tui::sigil_view::suggest;
use sigil::prelude::{Paint, node};

pub(super) fn layer(app: App) -> NodeBuilder {
    let submit = app.clone();
    let changed = app.clone();
    let surface = app.clone();
    let typeahead = app.clone();
    container()
        .bg(scene::SCRIM)
        .layout(LayoutStyle::block().fill_width().fill_height())
        .child(
            text_edit(app.draft)
                .state(app.editor.clone())
                .caret_position(app.caret)
                .content_padding(12.0, 0.0)
                .placeholder("")
                .chrome(false)
                .change(move |draft| {
                    changed.pane_focus.set(false);
                    changed.change(draft);
                })
                .submit(move |_| submit.submit())
                .layout(LayoutStyle::block().fill_width().fill_height()),
        )
        .child(scene::layer(surface))
        .child(node(move || popup(typeahead.clone())))
}

fn popup(app: App) -> NodeBuilder {
    let (cols, rows) = app.viewport.get();
    let [x, y, width] = scene::prompt_area(&app, cols, rows);
    let cell = canvas::cell();
    container()
        .layout(
            LayoutStyle::block()
                .position(Position::Absolute)
                .inset_left(f32::from(x) * cell.w)
                .inset_top(f32::from(y.saturating_add(1)) * cell.h),
        )
        .child(suggest::widget(app, usize::from(width)))
}
