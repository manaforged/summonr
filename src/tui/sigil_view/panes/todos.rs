use sigil::prelude::Paint;
use sigil::prelude::{LayoutBuilderExt, LayoutStyle, NodeBuilder};

use super::App;
use super::body::header;
use super::ledger::Mark;
use crate::tui::sigil_view::chat::todos::tally;

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
    for row in lines {
        let done = row.mark == Mark::Done;
        column = column.child(
            sigil::kernel::text(row.shown())
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
