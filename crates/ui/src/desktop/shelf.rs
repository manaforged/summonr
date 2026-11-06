use std::cell::Cell;

use sigil::prelude::{Signal, signal};
use summonr_agent::prefs::Layout;

/// The agents a person pinned or archived; the sidebar reads these, a restart keeps them.
#[derive(Clone, Copy)]
struct Shelf {
    pinned: Signal<Vec<String>>,
    archived: Signal<Vec<String>>,
}

thread_local! {
    static SHELF: Cell<Option<Shelf>> = const { Cell::new(None) };
}

/// Creates the shelf at the window's root, so no pane's scope owns it.
pub(super) fn install() {
    let made = Shelf {
        pinned: signal(Vec::new()),
        archived: signal(Vec::new()),
    };
    SHELF.with(|slot| slot.set(Some(made)));
}

fn shelf() -> Shelf {
    SHELF
        .with(Cell::get)
        .expect("invariant: the window installs the shelf before it builds a view")
}

/// Takes the pinned and archived agents from a saved layout.
pub(super) fn load(saved: &Layout) {
    let shelf = shelf();
    shelf.pinned.set(saved.pinned.clone());
    shelf.archived.set(saved.archived.clone());
}

/// The pinned agents, oldest pin first; tracked.
pub(super) fn pinned() -> Vec<String> {
    shelf().pinned.get()
}

/// The archived agents; tracked.
pub(super) fn archived() -> Vec<String> {
    shelf().archived.get()
}

/// Whether `id` is pinned; not tracked.
pub(super) fn is_pinned(id: &str) -> bool {
    shelf().pinned.peek().iter().any(|seen| seen == id)
}

/// Whether the repository lists leave `id` out, because it is pinned or archived; tracked.
pub(super) fn set_aside(id: &str) -> bool {
    let shelf = shelf();
    let held = |list: &Vec<String>| list.iter().any(|seen| seen == id);
    shelf.pinned.with(held) || shelf.archived.with(held)
}

/// Pins `id`, or unpins it when pinned.
pub(super) fn flip_pin(id: &str) {
    shelf().pinned.update(|list| flip(list, id));
}

/// Archives `id` and takes it off the pinned list.
pub(super) fn archive(id: &str) {
    let shelf = shelf();
    shelf.pinned.update(|list| list.retain(|seen| seen != id));
    shelf.archived.update(|list| {
        if !list.iter().any(|seen| seen == id) {
            list.push(id.to_owned());
        }
    });
}

/// Puts an archived `id` back in its repository.
pub(super) fn unarchive(id: &str) {
    if shelf().archived.peek().iter().any(|seen| seen == id) {
        shelf()
            .archived
            .update(|list| list.retain(|seen| seen != id));
    }
}

fn flip(list: &mut Vec<String>, id: &str) {
    if let Some(at) = list.iter().position(|seen| seen == id) {
        list.remove(at);
    } else {
        list.push(id.to_owned());
    }
}
