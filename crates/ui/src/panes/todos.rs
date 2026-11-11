use sigil::prelude::NodeBuilder;

use super::App;
use super::body::{Empty, empty, header};

const NO_LEDGER: Empty = Empty {
    window: "The agent's step-by-step plan shows here when it works on a task with several steps.",
    terminal: "no ledger items yet",
};
use super::ledger::Mark;
use crate::chat::todos::tally;
use crate::text_style::TextStyle;
use sigil::prelude::Token;

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
        return column.child(empty(&NO_LEDGER));
    }
    for row in lines {
        let tone = if row.mark == Mark::Done {
            Token::Gray11
        } else {
            Token::Gray12
        };
        column = column.child(TextStyle::xs(tone).note(row.shown()));
    }
    column
}
