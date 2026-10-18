use std::rc::Rc;

use sigil::prelude::Signal;
use sigil::reactive::{derived, untrack};

use super::super::{Entry, Model};
#[cfg(feature = "gui")]
use crate::prefs::Prefs;
use crate::tui::ToolState;
use crate::tui::sigil_view::model::Entries;

#[cfg(feature = "gui")]
pub(in crate::tui::sigil_view) type Content = (usize, u8, bool, Prefs, u64);

pub(in crate::tui::sigil_view) type Mark = (usize, u8, bool, u64);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::tui::sigil_view) enum Prose {
    #[cfg_attr(
        not(feature = "gui"),
        expect(dead_code, reason = "the desktop transcript remounts prose")
    )]
    Remount,
    Patch,
}

#[cfg(feature = "gui")]
pub(in crate::tui::sigil_view) fn content(entry: &Entry, live: bool, model: &Model) -> Content {
    let (size, state, live, kids) = mark(entry, live, children(entry, model), Prose::Remount);
    (size, state, live, model.prefs.clone(), kids)
}

pub(in crate::tui::sigil_view) fn revision(entries: &Entries, index: usize) -> u64 {
    match entries.get(index) {
        Some(Entry::Assistant { .. } | Entry::Thinking(_)) | None => 0,
        Some(_) => entries.rev(index),
    }
}

pub(in crate::tui::sigil_view) fn mark(entry: &Entry, live: bool, rev: u64, prose: Prose) -> Mark {
    let (size, state) = match entry {
        Entry::Assistant { text, done } => (text_size(text, prose), u8::from(*done)),
        Entry::Thinking(text) => (text_size(text, prose), 0),
        Entry::Tool {
            state,
            output,
            expanded,
            ..
        } => (output.len(), bits(state, *expanded)),
        Entry::ToolGroup { calls, .. } => {
            let size: usize = calls.iter().map(|call| call.output.len()).sum();
            let state = calls.iter().fold(0_u8, |state, call| {
                state
                    .wrapping_mul(31)
                    .wrapping_add(bits(&call.state, call.expanded))
            });
            (size, state)
        }
        Entry::User(_)
        | Entry::Receipt(_)
        | Entry::Verify { .. }
        | Entry::Mail { .. }
        | Entry::Aborted(_)
        | Entry::System(_)
        | Entry::Error(_)
        | Entry::Chart(_)
        | Entry::Image { .. } => (0, 0),
    };
    (size, state, live, rev)
}

fn bits(state: &ToolState, expanded: bool) -> u8 {
    u8::from(!matches!(state, ToolState::Running))
        + u8::from(matches!(state, ToolState::Failed))
        + u8::from(expanded) * 4
}

fn text_size(text: &str, prose: Prose) -> usize {
    match prose {
        Prose::Remount => text.len(),
        Prose::Patch => usize::from(!text.trim().is_empty()),
    }
}

#[cfg(feature = "gui")]
fn children(entry: &Entry, model: &Model) -> u64 {
    use std::hash::{Hash, Hasher};
    let Entry::Tool { id, .. } = entry else {
        return 0;
    };
    let mut hasher = std::hash::DefaultHasher::new();
    for kid in model
        .agents
        .iter()
        .filter(|kid| !id.is_empty() && kid.call == *id)
    {
        (&kid.id, &kid.last, kid.done, kid.failed, kid.secs).hash(&mut hasher);
    }
    hasher.finish()
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
