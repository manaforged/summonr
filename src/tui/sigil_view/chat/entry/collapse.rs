use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder, NodeEventsExt, Paint, PointerEvent,
    container,
};

use super::super::App;
use super::{prose, wrap};
use crate::prefs::Visibility;
use crate::tui::sigil_view::look::Look;

pub(super) fn collapsible(
    body: &str,
    kind: &str,
    visibility: Visibility,
    app: &App,
    reasoning: bool,
    live: bool,
) -> NodeBuilder {
    let toggle = app.clone();
    let next = if visibility == Visibility::Expanded {
        "collapsed"
    } else {
        "expanded"
    };
    let press = move |event: &mut PointerEvent| {
        if event.button == Some(MouseButton::Left) {
            if reasoning {
                toggle.reasoning_command(Some(next));
            } else {
                toggle.receipts_command(Some(next));
            }
            event.stop_propagation();
        }
    };
    match visibility {
        Visibility::Off | Visibility::Collapsed if live => wrap(
            sigil::kernel::text("✻ thinking…")
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .font_italic(true)
                .text_color(sigil::prelude::Token::Gray11),
        )
        .pointer_down(press),
        Visibility::Off => wrap(
            sigil::kernel::text(format!("▸ {kind} · hidden"))
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .font_italic(true)
                .text_color(sigil::prelude::Token::Gray11),
        )
        .pointer_down(press),
        Visibility::Collapsed if reasoning => {
            let words = body.split_whitespace().count();
            wrap(
                sigil::kernel::text(format!("▸ {kind} · {}", crate::text::plural(words, "word")))
                    .nowrap()
                    .text_color(sigil::prelude::Token::OnSurface)
                    .font_size(12.0)
                    .font_italic(true)
                    .text_color(sigil::prelude::Token::Gray11),
            )
            .pointer_down(press)
        }
        Visibility::Collapsed => {
            let count = body.lines().filter(|line| !line.trim().is_empty()).count();
            wrap(
                sigil::kernel::text(format!("▸ {kind} · {}", crate::text::plural(count, "line")))
                    .nowrap()
                    .text_color(sigil::prelude::Token::OnSurface)
                    .font_size(12.0)
                    .font_italic(true)
                    .text_color(sigil::prelude::Token::Gray11),
            )
            .pointer_down(press)
        }
        Visibility::Expanded => container()
            .layout(LayoutStyle::flex_column().fill_width().gap(0.0))
            .child(
                wrap(
                    sigil::kernel::text(if live {
                        "✻ thinking…".to_string()
                    } else {
                        format!("✻ {kind}")
                    })
                    .nowrap()
                    .text_color(sigil::prelude::Token::OnSurface)
                    .font_size(12.0)
                    .font_italic(true)
                    .text_color(sigil::prelude::Token::Gray11),
                )
                .pointer_down(press),
            )
            .child(prose(
                body,
                Look::xs(sigil::prelude::Token::Gray10).italic(),
            )),
    }
}
