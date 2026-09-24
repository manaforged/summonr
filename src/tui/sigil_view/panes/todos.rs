use sigil::prelude::{NodeBuilder, NodeStyleExt, classes, label};

use super::App;
use super::body::header;

pub(super) fn ledger(app: &App, mut column: NodeBuilder) -> NodeBuilder {
    let model = app.model.get();
    let count = match (model.plan_current, model.plan_total) {
        (Some(current), total) if total > 0 => format!("{current}/{total}"),
        _ => String::new(),
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
    let pad = " ".repeat(line.len() - line.trim_start().len());
    let body = line.trim_start();
    if let Some(rest) = body.strip_prefix("[x] ") {
        (format!("{pad}✓ {rest}"), true)
    } else if let Some(rest) = body.strip_prefix("[/] ") {
        (format!("{pad}● {rest}"), false)
    } else if let Some(rest) = body.strip_prefix("[ ] ") {
        (format!("{pad}○ {rest}"), false)
    } else if let Some(rest) = body.strip_prefix("[!] ") {
        (format!("{pad}! {rest}"), false)
    } else if let Some(rest) = body.strip_prefix("✓ ") {
        (format!("{pad}✓ {rest}"), true)
    } else if let Some(rest) = body.strip_prefix("● ") {
        (format!("{pad}● {rest}"), false)
    } else if let Some(rest) = body.strip_prefix("○ ") {
        (format!("{pad}○ {rest}"), false)
    } else {
        (line.to_string(), false)
    }
}
