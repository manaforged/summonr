use super::*;
use crate::tui::sigil_view::suggest;
use sigil::prelude::Paint;
use sigil::reactive::{derived, untrack};

pub(super) fn layer(app: App) -> NodeBuilder {
    let surface = app.clone();
    let shape = app.clone();
    let place = derived(move || {
        let (cols, rows) = shape.viewport.get();
        (rows, scene::window_area(&shape, cols, rows))
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
                    vec![untrack(|| window(app.clone(), rows, area))]
                }),
        )
}

fn window(app: App, rows: u16, [x, y, width, card_rows]: [u16; 4]) -> NodeBuilder {
    let cell = canvas::cell();
    let spacer = app.clone();
    let inner = width.saturating_sub(4);
    let tint = Rgba8 {
        a: (scene::GLASS * 255.0) as u8,
        ..scene::PLATE
    };
    let mut clear = surface::theme(&app.theme_name.peek(), app.backdrop.peek());
    clear.register_token(
        "surface",
        ThemeValue::Color(Rgba8 {
            a: 0,
            ..scene::PLATE
        }),
    );
    container()
        .layout(
            LayoutStyle::flex_column()
                .position(Position::Absolute)
                .inset_left(f32::from(x) * cell.w)
                .inset_top(f32::from(y) * cell.h)
                .width(f32::from(width) * cell.w)
                .max_height(f32::from(rows.saturating_sub(y)) * cell.h)
                .min_size(0.0, 0.0),
        )
        .theme(clear)
        .child(
            container()
                .layout(LayoutStyle::block().shrink(1.0).min_size(0.0, 0.0))
                .children_reactive(move || {
                    let list =
                        suggest::widget::height(&spacer, canvas::inner_cols(usize::from(inner)));
                    let open = card_rows.saturating_sub(list);
                    vec![container().layout(LayoutStyle::block().height(f32::from(open) * cell.h))]
                }),
        )
        .child(
            container()
                .layout(
                    LayoutStyle::flex_column()
                        .shrink(0.0)
                        .padding_x(cell.w * 2.0)
                        .padding_bottom(cell.h * 2.0)
                        .overflow(sigil::layout::Overflow::Hidden),
                )
                .bg(tint)
                .child(blank(width, rows.saturating_sub(y)))
                .child(chat::inline_layer(app.clone(), inner, surface::editor(app))),
        )
        .child(
            container()
                .layout(LayoutStyle::block().position(Position::Absolute).inset(0.0))
                .border_w(1)
                .rounded(sigil::prelude::Radius::Md)
                .border_color(sigil::prelude::Token::Gray8),
        )
}

fn blank(width: u16, rows: u16) -> NodeBuilder {
    let line = " ".repeat(usize::from(width));
    (0..rows).fold(
        container().layout(
            LayoutStyle::flex_column()
                .position(Position::Absolute)
                .inset(0.0)
                .gap(0.0),
        ),
        |column, _| {
            column.child(
                sigil::kernel::text(line.clone())
                    .nowrap()
                    .font_family(sigil::prelude::FontFamily::Mono),
            )
        },
    )
}
