use sigil::prelude::{AlignItems, LayoutStyle, NodeBuilder, Paint, Space, Token, container};

use super::super::canvas;
use crate::app::App;
use crate::text_style::TextStyle;
use sigil::prelude::Widget;

const CHIP_GAP_WINDOW: Space = Space::raw(14.0, "chip spacing between the medium and large steps");

pub(super) fn chips(app: &App) -> NodeBuilder {
    let active = app.picker.facet.get();
    let listed = app.picker.shown.get().len();
    let line = container().layout(
        LayoutStyle::flex_row()
            .fill_width()
            .height(canvas::overlay_list_row())
            .padding_x(Space::Sm)
            .gap(if canvas::is_window() {
                CHIP_GAP_WINDOW
            } else {
                Space::raw(canvas::cell().w, "one terminal cell between chips")
            })
            .align_items(AlignItems::Center),
    );
    app.facet_counts()
        .into_iter()
        .fold(line, |line, (facet, count)| {
            let on = facet == active;
            let count = if on { listed } else { count };
            let text_style = if on {
                TextStyle::sm(Token::Accent11).bold()
            } else {
                TextStyle::sm(Token::Gray11)
            };
            let chip = container().child(
                text_style
                    .on(sigil::prelude::text(format!(" {} {count} ", facet.label())).nowrap()),
            );
            let chip = if on {
                chip.bg(canvas::blend(
                    canvas::palette().base,
                    canvas::palette().accent,
                    0.22,
                ))
            } else {
                chip
            };
            let pick = app.clone();
            line.child(chip.pointer_down(move |event| {
                pick.pick_facet(facet);
                event.stop_propagation();
            }))
        })
}
