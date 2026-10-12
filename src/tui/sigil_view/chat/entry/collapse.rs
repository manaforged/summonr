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
        Visibility::Off | Visibility::Collapsed if live => {
            caption("✻ thinking…").pointer_down(press)
        }
        Visibility::Off if !reasoning => container(),
        Visibility::Off => caption(format!("▸ {kind} · hidden")).pointer_down(press),
        Visibility::Collapsed if reasoning => {
            let words = body.split_whitespace().count();
            caption(format!("▸ {kind} · {}", crate::text::plural(words, "word")))
                .pointer_down(press)
        }
        Visibility::Collapsed => {
            let count = body.lines().filter(|line| !line.trim().is_empty()).count();
            let gist = body
                .lines()
                .next()
                .and_then(|line| line.split_once(" · "))
                .map_or_else(
                    || crate::text::plural(count, "line"),
                    |(_, gist)| gist.to_string(),
                );
            caption(format!("▸ {kind} · {gist}")).pointer_down(press)
        }
        Visibility::Expanded => container()
            .layout(LayoutStyle::flex_column().fill_width().gap(0.0))
            .child(
                caption(if live {
                    "✻ thinking…".to_string()
                } else {
                    format!("✻ {kind}")
                })
                .pointer_down(press),
            )
            .child(prose(
                body,
                Look::xs(sigil::prelude::Token::Gray10).italic(),
            )),
    }
}

fn caption(text: impl Into<String>) -> NodeBuilder {
    wrap(
        sigil::kernel::text(text.into())
            .nowrap()
            .text_color(sigil::prelude::Token::OnSurface)
            .font_size(12.0)
            .font_italic(true)
            .text_color(sigil::prelude::Token::Gray11),
    )
}
