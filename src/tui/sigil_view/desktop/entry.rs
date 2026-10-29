use sigil::prelude::{
    LayoutBuilderExt, LayoutStyle, NodeBuilder, Signal, container, derived, signal,
};
use sigil::reactive::untrack;

use super::App;
use crate::prefs::Prefs;
use crate::tools::Builtin;
use crate::tui::sigil_view::chat::entry::{self as terminal, Content, Slot, content, slot};
use crate::tui::sigil_view::model::Entry;

mod child;
mod message;

type Version = (Slot, Vec<Content>, bool);

fn version(app: &App, index: usize) -> Version {
    app.model.with(|model| {
        let busy = model.busy_since.is_some();
        let last = model.entries.len();
        let at = slot(&model.entries, index);
        let range = at.span(index);
        let items = model
            .entries
            .get(range.clone())
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(offset, entry)| content(entry, busy && range.start + offset + 1 == last, model))
            .collect();
        (at, items, busy)
    })
}

pub(in crate::tui::sigil_view) fn row(app: App, index: usize, spacing: (f32, f32)) -> NodeBuilder {
    let probe = app.clone();
    let current = derived(move || version(&probe, index));
    let open: Signal<Option<bool>> = signal(None);
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .min_size(0.0, 0.0)
                .padding_x(spacing.0),
        )
        .children_reactive(move || {
            let (at, _, _) = current.get();
            let unfolded = open.get();
            untrack(|| match at {
                Slot::Member => Vec::new(),
                Slot::Lead(range) => vec![pad(
                    crate::tui::sigil_view::chat::entry::fold::card(&app, range, unfolded, open),
                    spacing.1,
                )],
                Slot::Single => single(&app, index)
                    .map(|node| vec![pad(node, spacing.1)])
                    .unwrap_or_default(),
            })
        })
}

fn pad(node: NodeBuilder, bottom: f32) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_column()
                .fill_width()
                .min_size(0.0, 0.0)
                .padding_bottom(bottom),
        )
        .child(node)
}

fn single(app: &App, index: usize) -> Option<NodeBuilder> {
    let (entry, live, prefs) = app.model.with(|model| model.entry_at(index));
    entry.map(|entry| form(entry, live, &prefs, app, index))
}

fn form(entry: Entry, live: bool, prefs: &Prefs, app: &App, index: usize) -> NodeBuilder {
    match entry {
        Entry::User(body) => message::user(body),
        Entry::Assistant { text, .. } => message::assistant(&text, app),
        Entry::Thinking(text) => message::thinking(&text, prefs.reasoning, live, app),
        Entry::System(body) => message::quiet(body),
        Entry::Aborted(reason) | Entry::Error(reason) => message::error(reason),
        Entry::Tool {
            ref name,
            ref calls,
        } if Builtin::is(name, Builtin::Task) => match child::cards(app, &calls[0].id) {
            Some(cards) => cards,
            None => terminal::row(entry, live, prefs, app, index),
        },
        entry @ (Entry::Tool { .. }
        | Entry::Receipt(_)
        | Entry::Verify { .. }
        | Entry::Mail { .. }
        | Entry::Chart(_)
        | Entry::Image { .. }) => terminal::row(entry, live, prefs, app, index),
    }
}
