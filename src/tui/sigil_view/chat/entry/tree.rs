use sigil::kernel::TextBuilder;
use sigil::prelude::Token;

use crate::tui::sigil_view::look::Look;
use sigil::prelude::{LayoutBuilderExt, LayoutStyle, NodeBuilder, container};

pub(in crate::tui::sigil_view) fn label(body: impl Into<String>, look: Look) -> TextBuilder {
    look.on(sigil::kernel::text(body.into()).nowrap())
}

pub(super) fn fill(body: impl Into<String>, look: Look) -> TextBuilder {
    label(body, look)
        .ellipsize()
        .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0))
}

pub(super) fn branch(first: bool, child: impl sigil::prelude::IntoNode + 'static) -> NodeBuilder {
    container()
        .layout(LayoutStyle::flex_row().fill_width().min_size(0.0, 0.0))
        .child(label(
            if first { "  ⎿ " } else { "    " },
            Look::xs(Token::Gray10),
        ))
        .child(child)
}

pub(super) fn tree(body: &str) -> NodeBuilder {
    let mut column = container().layout(LayoutStyle::flex_column().fill_width().gap(0.0));
    for (at, line) in body.lines().enumerate() {
        let tone = if line.starts_with("… +") {
            Look::xs(Token::Gray10)
        } else {
            Look::xs(Token::Gray11).mono()
        };
        column = column.child(branch(at == 0, fill(line.replace('\t', "    "), tone)));
    }
    column
}
