use sigil::kernel::{Cx, IntoNode, NodeId};
use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, MouseButton, NodeBuilder, NodeEventsExt, Paint, PointerEvent,
    Signal, container, markdown_inline,
};

use super::super::App;
use super::wrap;
use crate::prefs::Visibility;
use crate::tui::sigil_view::look::Look;

pub(super) fn collapsible(
    body: Signal<String>,
    kind: &'static str,
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
        Visibility::Collapsed => folded(body, kind, reasoning).pointer_down(press),
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

fn folded(body: Signal<String>, kind: &'static str, reasoning: bool) -> NodeBuilder {
    if reasoning {
        return caption(move || {
            let words = body.with(|body| body.split_whitespace().count());
            format!("▸ {kind} · {}", crate::text::plural(words, "word"))
        });
    }
    caption(move || body.with(|body| gist(kind, body)))
}

fn gist(kind: &str, body: &str) -> String {
    let count = body.lines().filter(|line| !line.trim().is_empty()).count();
    let gist = body
        .lines()
        .next()
        .and_then(|line| line.split_once(" · "))
        .map_or_else(
            || crate::text::plural(count, "line"),
            |(_, gist)| gist.to_string(),
        );
    format!("▸ {kind} · {gist}")
}

struct Line(String, Look);

impl IntoNode for Line {
    fn build(self, cx: &mut Cx) -> NodeId {
        self.1.on(markdown_inline(&self.0)).build(cx)
    }
}

fn prose(body: Signal<String>, look: Look) -> NodeBuilder {
    container()
        .layout(LayoutStyle::flex_column().fill_width().gap(0.0))
        .children_keyed(move || {
            body.with(|body| {
                body.split('\n')
                    .enumerate()
                    .map(|(index, para)| ((index, para.to_string()), Line(para.to_string(), look)))
                    .collect::<Vec<_>>()
            })
        })
}

fn caption(text: impl Into<sigil::kernel::TextContent>) -> NodeBuilder {
    wrap(
        sigil::kernel::text(text)
            .nowrap()
            .text_color(sigil::prelude::Token::OnSurface)
            .font_size(12.0)
            .font_italic(true)
            .text_color(sigil::prelude::Token::Gray11),
    )
}
