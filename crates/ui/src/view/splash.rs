use super::*;
use crate::suggest;
use sigil::prelude::Color;
use sigil::prelude::Paint;
use sigil::prelude::Space;
use sigil::prelude::Widget;
use sigil::reactive::{derived, untrack};
use sigil::style::ThemeValue;

pub(super) fn layer(app: App) -> NodeBuilder {
    let surface = app.clone();
    let shape = app.clone();
    let place = derived(move || {
        let (cols, rows) = shape.viewport.get();
        (rows, scene::window_area(&shape, cols, rows))
    });
    container()
        .background(scene::stage().scrim)
        .fill_width()
        .fill_height()
        .child(scene::layer(surface))
        .child(
            container()
                .position(Position::Absolute)
                .inset(0.0)
                .fill_width()
                .fill_height()
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
    let colours = scene::stage();
    let tint = Color {
        a: (colours.pane * 255.0) as u8,
        ..colours.plate
    };
    let mut clear = super::super::theme::style(super::super::theme::palette());
    clear.register_token(
        "surface",
        ThemeValue::Color(Color {
            a: 0,
            ..colours.plate
        }),
    );
    sigil::prelude::column()
        .gap(sigil::prelude::Space::None)
        .position(Position::Absolute)
        .inset_left(f32::from(x) * cell.w)
        .inset_top(f32::from(y) * cell.h)
        .width(f32::from(width) * cell.w)
        .max_height(f32::from(rows.saturating_sub(y)) * cell.h)
        .min_width(0.0)
        .min_height(0.0)
        .theme(clear)
        .child(
            container()
                .shrink(1.0)
                .min_width(0.0)
                .min_height(0.0)
                .children_reactive(move || {
                    let list =
                        suggest::widget::height(&spacer, canvas::inner_cols(usize::from(inner)));
                    let open = card_rows.saturating_sub(list);
                    vec![container().height(f32::from(open) * cell.h)]
                }),
        )
        .child(
            sigil::prelude::column()
                .gap(sigil::prelude::Space::None)
                .shrink(0.0)
                .padding_x(Space::raw(cell.w * 2.0, "two terminal cells"))
                .padding_bottom(Space::raw(cell.h * 2.0, "two terminal rows"))
                .overflow(sigil::prelude::Overflow::Hidden)
                .background(tint)
                .child(blank(width, rows.saturating_sub(y)))
                .child(chat::inline_layer(
                    app.clone(),
                    inner,
                    surface::terminal_editor(app),
                )),
        )
        .child(
            container()
                .position(Position::Absolute)
                .inset(0.0)
                .border_width(1)
                .radius(sigil::prelude::Radius::Md)
                .border_color(sigil::prelude::Token::Gray8),
        )
}

fn blank(width: u16, rows: u16) -> NodeBuilder {
    let line = " ".repeat(usize::from(width));
    (0..rows).fold(
        sigil::prelude::column()
            .position(Position::Absolute)
            .inset(0.0)
            .gap(Space::None),
        |column, _| {
            column.child(
                sigil::prelude::text(line.clone())
                    .nowrap()
                    .font_family(sigil::prelude::FontFamily::Mono),
            )
        },
    )
}
