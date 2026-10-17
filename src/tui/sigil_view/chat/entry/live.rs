use std::rc::Rc;

use sigil::prelude::Signal;
use sigil::reactive::derived;

use super::super::{Entry, Model};
#[cfg(feature = "gui")]
use crate::prefs::Prefs;
use crate::tui::ToolState;

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
    let (size, state, live, kids) = mark(entry, live, model, Prose::Remount);
    (size, state, live, model.prefs.clone(), kids)
}

pub(in crate::tui::sigil_view) fn mark(
    entry: &Entry,
    live: bool,
    model: &Model,
    prose: Prose,
) -> Mark {
    let (size, state) = match entry {
        Entry::Assistant { text, done } => (text_size(text, prose), u8::from(*done)),
        Entry::Thinking(text) => (text_size(text, prose), 0),
        Entry::Tool {
            state,
            output,
            expanded,
            ..
        } => (
            output.len(),
            u8::from(!matches!(state, ToolState::Running))
                + u8::from(matches!(state, ToolState::Failed))
                + u8::from(*expanded) * 4,
        ),
        Entry::ToolGroup { calls, .. } => {
            let size: usize = calls.iter().map(|call| call.output.len()).sum();
            let mut state = 0_u8;
            for call in calls {
                state |= u8::from(!matches!(call.state, ToolState::Running));
                state |= u8::from(matches!(call.state, ToolState::Failed)) * 2;
                state |= u8::from(call.expanded) * 4;
            }
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
    (size, state, live, children(entry, model))
}

fn text_size(text: &str, prose: Prose) -> usize {
    match prose {
        Prose::Remount => text.len(),
        Prose::Patch => usize::from(!text.trim().is_empty()),
    }
}

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
    derived(move || {
        model.with(|model| match model.entries.get(index) {
            Some(Entry::Assistant { text, .. } | Entry::Thinking(text)) => text.clone(),
            _ => String::new(),
        })
    })
}
