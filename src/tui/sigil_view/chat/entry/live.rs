use std::rc::Rc;

use sigil::prelude::Signal;
use sigil::reactive::{derived, untrack};

use super::super::{Entry, Model, canvas};
use crate::tui::ToolState;
use crate::tui::sigil_view::model::Entries;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::tui::sigil_view) enum Revision {
    Entry(u64),
    Assistant { empty: bool, done: bool },
    Thinking { empty: bool },
}

pub(in crate::tui::sigil_view) fn revision(entries: &Entries, index: usize) -> Revision {
    if !canvas::is_window() {
        match entries.get(index) {
            Some(Entry::Assistant { text, done }) => {
                return Revision::Assistant {
                    empty: text.trim().is_empty(),
                    done: *done,
                };
            }
            Some(Entry::Thinking(text)) => {
                return Revision::Thinking {
                    empty: text.trim().is_empty(),
                };
            }
            _ => {}
        }
    }
    Revision::Entry(entries.rev(index))
}

pub(in crate::tui::sigil_view) fn bits(state: &ToolState, expanded: bool) -> u8 {
    u8::from(!matches!(state, ToolState::Running))
        + u8::from(matches!(state, ToolState::Failed))
        + u8::from(expanded) * 4
}

pub(in crate::tui::sigil_view) fn text_of(
    model: Signal<Rc<Model>>,
    index: usize,
) -> Signal<String> {
    let rev = derived(move || model.with(|model| model.entries.rev(index)));
    derived(move || {
        rev.with(|_| ());
        untrack(|| {
            model.with(|model| match model.entries.get(index) {
                Some(Entry::Assistant { text, .. } | Entry::Thinking(text)) => text.clone(),
                _ => String::new(),
            })
        })
    })
}
