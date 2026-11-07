use sigil::prelude::Space;
use sigil::prelude::{
    LayoutStyle, MouseButton, NodeBuilder, Paint, PointerEvent, Signal, container, markdown_inline,
};
use sigil::{Cx, NodeId, prelude::IntoNode};

use super::super::App;
use super::wrap;
use crate::text_style::TextStyle;
use sigil::prelude::Widget;
use summonr_agent::prefs::Visibility;

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
        Visibility::Collapsed
    } else {
        Visibility::Expanded
    };
    let field: fn(&mut summonr_agent::prefs::Prefs) -> &mut Visibility = if reasoning {
        |prefs| &mut prefs.reasoning
    } else {
        |prefs| &mut prefs.receipts
    };
    let press = move |event: &mut PointerEvent| {
        if event.button == Some(MouseButton::Left) {
            toggle.set_visibility(field, Some(next));
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
            .layout(LayoutStyle::flex_column().fill_width().gap(Space::None))
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
                TextStyle::xs(sigil::prelude::Token::Gray10).italic(),
            )),
    }
}

fn folded(body: Signal<String>, kind: &'static str, reasoning: bool) -> NodeBuilder {
    if reasoning {
        return caption(move || {
            let words = body.with(|body| body.split_whitespace().count());
            format!("▸ {kind} · {}", summonr_core::text::plural(words, "word"))
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
            || summonr_core::text::plural(count, "line"),
            |(_, gist)| gist.to_string(),
        );
    format!("▸ {kind} · {gist}")
}

struct Line(String, TextStyle);

impl IntoNode for Line {
    fn build(self, cx: &mut Cx) -> NodeId {
        self.1.on(markdown_inline(&self.0)).build(cx)
    }
}

fn prose(body: Signal<String>, text_style: TextStyle) -> NodeBuilder {
    container()
        .layout(LayoutStyle::flex_column().fill_width().gap(Space::None))
        .children_keyed(move || {
            body.with(|body| {
                body.split('\n')
                    .enumerate()
                    .map(|(index, para)| {
                        (
                            (index, para.to_string()),
                            Line(para.to_string(), text_style),
                        )
                    })
                    .collect::<Vec<_>>()
            })
        })
}

fn caption(text: impl Into<sigil::prelude::TextContent>) -> NodeBuilder {
    wrap(
        sigil::prelude::text(text)
            .nowrap()
            .font_size(
                12.0,
                "12 px chrome text that keeps its own line height and weight",
            )
            .font_italic(true)
            .text_color(sigil::prelude::Token::Gray11),
    )
}
