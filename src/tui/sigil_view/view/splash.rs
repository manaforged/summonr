use super::*;
use crate::tui::sigil_view::suggest;
use sigil::prelude::Paint;
use sigil::reactive::{derived, untrack};

pub(super) fn layer(app: App) -> NodeBuilder {
    let surface = app.clone();
    let shape = app.clone();
    let place = derived(move || {
        let (cols, rows) = shape.viewport.get();
        (rows, scene::prompt_area(&shape, cols, rows))
    });
    container()
        .bg(scene::SCRIM)
        .layout(LayoutStyle::block().fill_width().fill_height())
        .child(scene::layer(surface))
        .child(
            container()
                .layout(
                    LayoutStyle::block()
                        .position(Position::Absolute)
                        .inset(0.0)
                        .fill_width()
                        .fill_height(),
                )
                .children_reactive(move || {
                    let (rows, area) = place.get();
                    vec![untrack(|| composer(app.clone(), rows, area))]
                }),
        )
}

fn composer(app: App, rows: u16, [x, y, width]: [u16; 3]) -> NodeBuilder {
    let cell = canvas::cell();
    let spacer = app.clone();
    let mut plate = surface::theme(&app.theme_name.peek(), app.backdrop.peek());
    plate.register_token("surface", ThemeValue::Color(scene::PLATE));
    container()
        .layout(
            LayoutStyle::flex_column()
                .position(Position::Absolute)
                .inset_left(f32::from(x) * cell.w)
                .inset_top(0.0)
                .width(f32::from(width) * cell.w)
                .height(f32::from(rows) * cell.h)
                .min_size(0.0, 0.0),
        )
        .child(
            container()
                .layout(LayoutStyle::block().shrink(1.0).min_size(0.0, 0.0))
                .children_reactive(move || {
                    let list =
                        suggest::widget::height(&spacer, canvas::inner_cols(usize::from(width)));
                    let top = y.saturating_sub(list).min(rows.saturating_sub(4));
                    vec![container().layout(LayoutStyle::block().height(f32::from(top) * cell.h))]
                }),
        )
        .child(
            container()
                .layout(LayoutStyle::flex_column().fill_width().shrink(0.0))
                .bg(scene::PLATE)
                .theme(plate)
                .child(chat::inline_layer(app.clone(), width, surface::editor(app))),
        )
}
