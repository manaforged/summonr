use sigil::prelude::Space;
use sigil::prelude::TextBuilder;
use sigil::prelude::Token;

use crate::text_style::TextStyle;
use sigil::prelude::Widget;
use sigil::prelude::{LayoutStyle, NodeBuilder, container};

pub(crate) fn label(body: impl Into<String>, text_style: TextStyle) -> TextBuilder {
    text_style.on(sigil::prelude::text(body.into()).nowrap())
}

pub(super) fn fill(body: impl Into<String>, text_style: TextStyle) -> TextBuilder {
    label(body, text_style)
        .ellipsize()
        .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0))
}

pub(super) fn branch(first: bool, child: impl sigil::prelude::IntoNode + 'static) -> NodeBuilder {
    container()
        .layout(LayoutStyle::flex_row().fill_width().min_size(0.0, 0.0))
        .child(label(
            if first { "  ⎿ " } else { "    " },
            TextStyle::xs(Token::Gray10),
        ))
        .child(child)
}

pub(super) fn tree(body: &str) -> NodeBuilder {
    let mut column = container().layout(LayoutStyle::flex_column().fill_width().gap(Space::None));
    for (at, line) in body.lines().enumerate() {
        let tone = if line.starts_with("… +") {
            TextStyle::xs(Token::Gray10)
        } else {
            TextStyle::xs(Token::Gray11).mono()
        };
        column = column.child(branch(at == 0, fill(line.replace('\t', "    "), tone)));
    }
    column
}
