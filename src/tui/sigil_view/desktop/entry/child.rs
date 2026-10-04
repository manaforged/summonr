use sigil::prelude::{
    AlignItems, LayoutBuilderExt, LayoutStyle, NodeBuilder, Paint, Radius, Token, container,
};

use super::super::App;
use crate::tui::sigil_view::chat::entry::{glyph, label};
use crate::tui::sigil_view::look::Look;
use crate::tui::sigil_view::model::Agent;

fn card(kid: &Agent) -> NodeBuilder {
    let (dot, tone) = glyph(&kid.state());
    let mode = if kid.background {
        "background"
    } else {
        "foreground"
    };
    let meta = format!(
        "{mode} · {} · {} tok",
        kid.clock(),
        crate::provider::compact(kid.tokens)
    );
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .gap(10.0)
                .padding_x(12.0)
                .padding_y(8.0)
                .min_size(0.0, 0.0)
                .align_items(AlignItems::Center),
        )
        .border_w(1)
        .border_color(Token::BorderSubtle)
        .rounded(Radius::Md)
        .child(label(dot, tone))
        .child(
            label(
                format!("Child task · {}", kid.title),
                Look::sm(Token::Gray11),
            )
            .ellipsize()
            .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0)),
        )
        .child(label(meta, Look::xs(Token::Gray10)))
}

pub(super) fn cards(app: &App, call: &str) -> Option<NodeBuilder> {
    let model = app.model.get();
    let kids: Vec<&Agent> = model
        .agents
        .iter()
        .filter(|kid| !call.is_empty() && kid.call == call)
        .collect();
    if kids.is_empty() {
        return None;
    }
    let mut column = container().layout(LayoutStyle::flex_column().fill_width().gap(8.0));
    for kid in kids {
        column = column.child(card(kid));
    }
    Some(column)
}
