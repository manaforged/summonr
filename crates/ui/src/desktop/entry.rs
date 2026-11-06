use sigil::prelude::NodeBuilder;

use super::App;
use crate::chat::entry as terminal;
use crate::model::Entry;
use summonr_agent::prefs::Prefs;

mod agents;
pub(crate) mod fold;
mod message;
mod verb;

pub(crate) use agents::group as agents;

pub(crate) fn single(app: &App, index: usize) -> Option<NodeBuilder> {
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
        Entry::User(_)
        | Entry::Receipt(_)
        | Entry::Tool { .. }
        | Entry::Verify { .. }
        | Entry::Mail { .. }
        | Entry::Aborted(_)
        | Entry::Error(_)
        | Entry::Chart(_)
        | Entry::Image { .. } => false,
    }
}

fn form(entry: Entry, live: bool, prefs: &Prefs, app: &App, index: usize) -> NodeBuilder {
    match entry {
        Entry::User(body) => message::user(body),
        Entry::Assistant { text, .. } => message::assistant(&text, app),
        Entry::Thinking(text) => message::thinking(&text, prefs.reasoning, live, app),
        Entry::System(_) | Entry::Injected(_) => sigil::prelude::container(),
        Entry::Aborted(reason) => message::error(message::first_part(&reason), None),
        Entry::Error(reason) => {
            let last = app.model.with(|model| model.entries.len() == index + 1);
            match reason.strip_suffix(summonr_agent::hub::RETRY_HINT) {
                Some(line) => message::error(message::first_line(line), last.then_some(app)),
                None => message::error(message::first_line(&reason), None),
            }
        }
        entry @ (Entry::Tool { .. }
        | Entry::Receipt(_)
        | Entry::Verify { .. }
        | Entry::Mail { .. }
        | Entry::Chart(_)
        | Entry::Image { .. }) => terminal::row(entry, live, prefs, app, index),
    }
}

pub(crate) fn diff_card(app: &App, text: &str) -> Option<NodeBuilder> {
    let layout = app.model.get().prefs.diff_layout;
    terminal::reply::classify(text).map(|shown| terminal::reply::paint(shown, app, layout))
}
