use sigil::prelude::{NodeBuilder, NodeStyleExt, classes, label};

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
    column = column.child(header(app, "Todos".to_string(), count));
    if lines.is_empty() {
        return column.child(label("no todos yet").class(classes!("text-xs text-gray-11")));
    }
    for line in lines {
        let (shown, done) = plan_mark(&line);
        column = column.child(label(shown).class(if done {
            classes!("text-xs text-gray-11")
        } else {
            classes!("text-xs text-gray-12")
        }));
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
