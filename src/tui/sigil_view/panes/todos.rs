use sigil::prelude::Paint;
use sigil::prelude::{LayoutBuilderExt, LayoutStyle, NodeBuilder};

use super::App;
use super::body::header;
use super::ledger::item;
use crate::tui::sigil_view::chat::todos::{glyph, tally};

pub(super) fn ledger(app: &App, mut column: NodeBuilder) -> NodeBuilder {
    let model = app.model.get();
    let count = match tally(&model.ledger) {
        (_, 0) => String::new(),
        (done, total) => format!("{done}/{total}"),
    };
    let lines = model.ledger.clone();
    drop(model);
    column = column.child(header(app, "Ledger".to_string(), count));
    if lines.is_empty() {
        return column.child(
            sigil::kernel::text("no ledger items yet")
                .nowrap()
                .text_color(sigil::prelude::Token::OnSurface)
                .font_size(12.0)
                .text_color(sigil::prelude::Token::Gray11),
        );
    }
    for line in lines {
        let (shown, done) = plan_mark(&line);
        column = column.child(
            sigil::kernel::text(shown)
                .wrap()
                .layout(LayoutStyle::block().fill_width().min_size(0.0, 14.0))
                .font_size(12.0)
                .text_color(if done {
                    sigil::prelude::Token::Gray11
                } else {
                    sigil::prelude::Token::Gray12
                }),
        );
    }
    column
}

pub(super) fn plan_mark(line: &str) -> (String, bool) {
    let (mark, rest) = item(line);
    if mark == ' ' {
        return (line.to_string(), false);
    }
    let pad = &line[..line.len() - line.trim_start().len()];
    (format!("{pad}{} {rest}", glyph(mark)), mark == '✓')
}
