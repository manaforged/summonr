use sigil::prelude::NodeBuilder;

use super::App;
use crate::prefs::Prefs;
use crate::tui::sigil_view::chat::entry as terminal;
use crate::tui::sigil_view::model::Entry;

mod agents;
mod message;

pub(in crate::tui::sigil_view) use agents::group as agents;

pub(in crate::tui::sigil_view) fn single(app: &App, index: usize) -> Option<NodeBuilder> {
    let (entry, live, prefs) = app.model.with(|model| model.entry_at(index));
    entry
        .filter(|entry| !hidden(entry))
        .map(|entry| form(entry, live, &prefs, app, index))
}

fn hidden(entry: &Entry) -> bool {
    match entry {
        Entry::Injected(_) | Entry::System(_) => true,
        Entry::Assistant { text, .. } => text.trim().is_empty(),
        Entry::Thinking(text) => text.trim().is_empty(),
        _ => false,
    }
}

fn form(entry: Entry, live: bool, prefs: &Prefs, app: &App, index: usize) -> NodeBuilder {
    match entry {
        Entry::User(body) => message::user(body),
        Entry::Assistant { text, .. } => message::assistant(&text, app),
        Entry::Thinking(text) => message::thinking(&text, prefs.reasoning, live, app),
        Entry::System(_) | Entry::Injected(_) => sigil::prelude::container(),
        Entry::Aborted(reason) | Entry::Error(reason) => message::error(reason),
        entry @ (Entry::Tool { .. }
        | Entry::Receipt(_)
        | Entry::Verify { .. }
        | Entry::Mail { .. }
        | Entry::Chart(_)
        | Entry::Image { .. }) => terminal::row(entry, live, prefs, app, index),
    }
}
