use super::*;
use sigil::prelude::{Paint, node};

pub(super) fn layer(app: App) -> NodeBuilder {
    let surface = app.clone();
    container()
        .bg(scene::SCRIM)
        .layout(LayoutStyle::block().fill_width().fill_height())
        .child(scene::layer(surface))
        .child(node(move || {
            let (cols, rows) = app.viewport.get();
            let [x, y, width] = scene::prompt_area(&app, cols, rows);
            let cell = canvas::cell();
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .position(Position::Absolute)
                        .inset_left(f32::from(x) * cell.w)
                        .inset_bottom(f32::from(rows.saturating_sub(y + 4)) * cell.h)
                        .width(f32::from(width) * cell.w)
                        .min_size(0.0, 0.0),
                )
                .child(chat::inline_layer(
                    app.clone(),
                    width,
                    surface::editor(app.clone()),
                ))
        }))
}
