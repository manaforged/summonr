use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

use sigil::prelude::{Signal, effect, signal};
use summonr_agent::hub::{Live, Open, Status};
use summonr_agent::prefs::{Layout, Mark, Slot};

use super::App;
use super::terminal::Term;
use crate::model::Entry;

type Marks = BTreeMap<String, Mark>;

thread_local! {
    static MARKS: Cell<Option<Signal<Marks>>> = const { Cell::new(None) };
}

/// Reopens the saved panes and terminals, then saves each later change.
pub(super) fn install(main: &App) {
    if main.prefs_path.is_none() {
        return;
    }
    let saved = Layout::load();
    super::shelf::load(&saved);
    super::spaces::load(&saved);
    let marks = signal(saved.marks.clone());
    MARKS.with(|slot| slot.set(Some(marks)));
    reopen(main, &saved);
    watch(main, marks, saved);
}

/// The status saved for an agent that is not running; tracked.
pub(super) fn mark(session: &str) -> Option<Status> {
    let marks = MARKS.with(Cell::get)?;
    marks
        .with(|marks| marks.get(session).copied())
        .map(|mark| match mark {
            Mark::Review => Status::Review,
            Mark::Failed => Status::Failed,
        })
}

/// Marks an agent that is not running unread; a running one takes its status from the hub.
pub(super) fn mark_unread(session: &str) {
    if let Some(marks) = MARKS.with(Cell::get) {
        marks.update(|marks| {
            marks.insert(session.to_owned(), Mark::Review);
        });
    }
}

fn reopen(main: &App, saved: &Layout) {
    let terminals: Vec<Option<u64>> = saved
        .terminals
        .iter()
        .map(|folder| main.window.terminals.reopen(folder))
        .collect();
    for (at, slot) in saved.panes.iter().enumerate() {
        let pane = if at == 0 {
            if !slot.session.is_empty() {
                main.open_session(Open::Resume(slot.session.clone()));
            }
            Some(main.clone())
        } else {
            super::split::reopen(&slot.session)
        };
        let shown = slot
            .terminal
            .and_then(|at| terminals.get(at).copied().flatten());
        if let (Some(pane), Some(id)) = (pane, shown) {
            pane.window.showing.set(Some(id));
        }
    }
    super::split::arrange(saved.shape.clone());
    super::split::focus_at(saved.focus);
}

fn watch(main: &App, marks: Signal<Marks>, saved: Layout) {
    let (main, last) = (main.clone(), Rc::new(std::cell::RefCell::new(saved)));
    effect(move || {
        let Some(layout) = snapshot(&main, &marks.get()) else {
            return;
        };
        if *last.borrow() == layout {
            return;
        }
        marks.set(layout.marks.clone());
        *last.borrow_mut() = layout.clone();
        let tell = main.clone();
        sigil::reactive::defer(move || {
            if let Err(error) = layout.save() {
                tell.push_entry(Entry::Error(format!("Could not save the window: {error}")));
            }
        });
    });
}

fn snapshot(main: &App, marks: &Marks) -> Option<Layout> {
    let (apps, focus) = super::split::tracked();
    let terms = main.window.terminals.list.get();
    let panes: Vec<Slot> = apps
        .iter()
        .map(|app| Slot {
            session: app.model.with(|model| model.session.clone()),
            terminal: position(&terms, app.window.showing.get()),
        })
        .collect();
    if panes.first().is_none_or(|slot| slot.session.is_empty()) {
        return None;
    }
    let live = main.model.with(|model| model.live.clone());
    let chats: Vec<String> = main
        .sessions
        .all_chats()
        .into_iter()
        .map(|chat| chat.id)
        .collect();
    Some(Layout {
        panes,
        shape: Some(super::split::shape()),
        focus,
        terminals: terms.iter().map(|term| term.folder.clone()).collect(),
        marks: next_marks(marks, &live, &chats),
        pinned: kept(super::shelf::pinned(), &chats),
        archived: kept(super::shelf::archived(), &chats),
        spaces: super::spaces::kept(&chats),
    })
}

fn kept(mut list: Vec<String>, chats: &[String]) -> Vec<String> {
    if !chats.is_empty() {
        list.retain(|id| chats.contains(id));
    }
    list
}

fn position(terms: &[Rc<Term>], shown: Option<u64>) -> Option<usize> {
    let shown = shown?;
    terms.iter().position(|term| term.id == shown)
}

fn next_marks(marks: &Marks, live: &[Live], chats: &[String]) -> Marks {
    let mut next = marks.clone();
    if !chats.is_empty() {
        next.retain(|session, _| chats.contains(session));
    }
    for entry in live {
        match entry.status {
            Status::Idle => next.remove(&entry.session),
            Status::Failed => next.insert(entry.session.clone(), Mark::Failed),
            Status::Working | Status::NeedsYou | Status::Review => {
                next.insert(entry.session.clone(), Mark::Review)
            }
        };
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(session: &str, status: Status) -> Live {
        Live {
            session: session.to_owned(),
            title: String::new(),
            workspace: "/work".into(),
            status,
            active: false,
            parent: None,
        }
    }

    #[test]
    fn marks_follow_live_agents_and_outlast_closed_ones() {
        let before = Marks::from([
            ("closed".to_owned(), Mark::Failed),
            ("seen".to_owned(), Mark::Review),
            ("deleted".to_owned(), Mark::Review),
        ]);
        let now = [
            live("seen", Status::Idle),
            live("cut", Status::Working),
            live("broke", Status::Failed),
        ];
        let chats = ["closed", "seen", "cut", "broke"].map(str::to_owned);
        assert_eq!(
            next_marks(&before, &now, &chats),
            Marks::from([
                ("broke".to_owned(), Mark::Failed),
                ("closed".to_owned(), Mark::Failed),
                ("cut".to_owned(), Mark::Review),
            ])
        );
    }
}
