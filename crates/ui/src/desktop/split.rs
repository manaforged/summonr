use std::cell::RefCell;

use sigil::prelude::{Signal, signal};
use sigil::reactive::Owner;

use super::App;
use crate::view::keys::bindings::Split as Act;
use summonr_agent::hub::{Cmd, Open};
use summonr_agent::prefs::{Axis, Shape};

/// A pane's place on screen: x, y, width and height.
type Bounds = (f32, f32, f32, f32);

/// One pane: its agent's view, the owner that holds the pane's effects, and where it is drawn.
struct Pane {
    app: App,
    owner: Option<Owner>,
    bounds: Signal<Bounds>,
}

mod add;
mod arrange;
mod layout;
use add::add;
pub(super) use arrange::{arrange, shape};
pub(super) use layout::view;

struct Split {
    panes: Vec<Pane>,
    shape: Shape,
    zoom: Option<usize>,
    focus: Signal<usize>,
    revision: Signal<u64>,
    resized: Signal<u64>,
    root: Option<Owner>,
}

thread_local! {
    static SPLIT: RefCell<Option<Split>> = const { RefCell::new(None) };
}

/// Makes the window's first pane from `main`.
pub(super) fn install(main: &App) {
    let split = Split {
        panes: vec![Pane {
            app: main.clone(),
            owner: None,
            bounds: signal((0.0, 0.0, 0.0, 0.0)),
        }],
        shape: Shape::default(),
        zoom: None,
        focus: signal(0),
        revision: signal(0),
        resized: signal(0),
        root: Owner::current(),
    };
    SPLIT.with(|slot| *slot.borrow_mut() = Some(split));
}

fn with<R>(f: impl FnOnce(&mut Split) -> R) -> Option<R> {
    SPLIT.with(|slot| slot.borrow_mut().as_mut().map(f))
}

/// The pane that takes keys, when the window is split and it is not the first.
pub(crate) fn focused() -> Option<App> {
    with(|split| {
        let at = split.focus.peek();
        (at > 0)
            .then(|| split.panes.get(at).map(|pane| pane.app.clone()))
            .flatten()
    })
    .flatten()
}

/// The focused pane's app, or `main` when the window is not split; tracked.
pub(super) fn current(main: &App) -> App {
    let Some((focus, revision)) = with(|split| (split.focus, split.revision)) else {
        return main.clone();
    };
    let (at, _) = (focus.get(), revision.get());
    with(|split| split.panes.get(at).map(|pane| pane.app.clone()))
        .flatten()
        .unwrap_or_else(|| main.clone())
}

/// Every pane's app.
pub(super) fn apps() -> Vec<App> {
    with(|split| split.panes.iter().map(|pane| pane.app.clone()).collect()).unwrap_or_default()
}

/// Whether any pane shows the agent `id`.
pub(super) fn shows(id: &str) -> bool {
    with(|split| {
        split
            .panes
            .iter()
            .any(|pane| pane.app.model.peek().session == id)
    })
    .unwrap_or(false)
}

/// The chats showing in any pane; tracked.
pub(super) fn shown_chats() -> Vec<String> {
    tracked()
        .0
        .iter()
        .filter(|app| app.window.showing.get().is_none())
        .map(|app| app.model.with(|model| model.session.clone()))
        .collect()
}

/// Opens a new pane on the chat `session`; false when the window has no room.
pub(super) fn beside(session: &str) -> bool {
    reopen(session).is_some()
}

/// Opens a new pane with a new chat in `folder`; false when the window has no room.
pub(super) fn beside_in(folder: std::path::PathBuf) -> bool {
    add(Axis::Right, move |_| Open::Folder(folder)).is_some()
}

/// Closes the pane that shows the chat `session`.
pub(super) fn close_chat(session: &str) {
    let at = with(|split| {
        split
            .panes
            .iter()
            .position(|pane| pane.app.model.peek().session == session)
    })
    .flatten();
    if let Some(at) = at {
        close(at);
    }
}

/// Whether more than one pane is open.
pub(super) fn multiple() -> bool {
    with(|split| split.panes.len() > 1).unwrap_or(false)
}

/// Whether more than one pane is open; tracked.
pub(super) fn active() -> bool {
    with(|split| split.revision).is_some_and(|revision| {
        revision.with(|_| ());
        multiple()
    })
}

/// Whether `app` is the window's first pane.
pub(super) fn is_first(app: &App) -> bool {
    with(|split| {
        split
            .panes
            .first()
            .is_some_and(|pane| pane.app.cmd.same_channel(&app.cmd))
    })
    .unwrap_or(true)
}

/// Runs a split key.
pub(crate) fn act(action: Act) {
    match action {
        Act::Open => open(),
        Act::Close => {
            if !hide_terminal() {
                close_focused();
            }
        }
        Act::Below => drop(add(Axis::Down, Open::Folder)),
        Act::Focus(dir) => arrange::go(dir),
        Act::Swap(dir) => arrange::swap(dir),
        Act::Zoom => arrange::zoom(),
    }
}

/// Opens `open` in the focused pane, or focuses the pane that already shows that agent.
pub(super) fn show(main: &App, open: Open) {
    if let Open::Resume(id) = &open
        && let Some(at) = with(|split| {
            split
                .panes
                .iter()
                .position(|pane| pane.app.model.peek().session == *id)
        })
        .flatten()
    {
        if let Some(app) = with(|split| split.panes.get(at).map(|pane| pane.app.clone())).flatten()
        {
            app.window.showing.set(None);
        }
        return focus(at);
    }
    focused().unwrap_or_else(|| main.clone()).open_session(open);
}

/// Focuses the pane that shows terminal `id`; false when none does.
pub(super) fn focus_terminal(id: u64) -> bool {
    let at = with(|split| {
        split
            .panes
            .iter()
            .position(|pane| pane.app.window.showing.peek() == Some(id))
    })
    .flatten();
    at.inspect(|at| focus(*at)).is_some()
}

/// Puts the focused pane back on its agent; false when it shows no terminal.
fn hide_terminal() -> bool {
    let pane = with(|split| {
        split
            .panes
            .get(split.focus.peek())
            .map(|pane| pane.app.clone())
    })
    .flatten();
    pane.is_some_and(|app| {
        let shows = app.window.showing.peek().is_some();
        if shows {
            app.window.showing.set(None);
        }
        shows
    })
}

fn open() {
    drop(add(Axis::Right, Open::Folder));
}

/// Opens a pane on the saved agent `session`.
pub(super) fn reopen(session: &str) -> Option<App> {
    let session = session.to_owned();
    add(Axis::Right, move |_| Open::Resume(session))
}

/// Focuses pane `at`, or the last pane when it no longer exists.
pub(super) fn focus_at(at: usize) {
    if let Some(last) = with(|split| split.panes.len().saturating_sub(1)) {
        focus(at.min(last));
    }
}

/// Every pane's app and the focused index; tracked.
pub(super) fn tracked() -> (Vec<App>, usize) {
    let Some((focus, revision)) = with(|split| (split.focus, split.revision)) else {
        return (Vec::new(), 0);
    };
    let (at, _) = (focus.get(), revision.get());
    (apps(), at)
}

fn close_focused() {
    let Some(at) = with(|split| (split.panes.len() > 1).then(|| split.focus.peek())).flatten()
    else {
        return;
    };
    close(at);
}

/// Closes the pane that shows `app`.
pub(super) fn close_app(app: &App) {
    if let Some(at) = with(|split| {
        split
            .panes
            .iter()
            .position(|pane| pane.app.cmd.same_channel(&app.cmd))
    })
    .flatten()
    {
        close(at);
    }
}

/// Closes pane `at`; closing the first pane moves the second pane's agent into it.
pub(super) fn close(at: usize) {
    let Some((main, second)) = with(|split| {
        (split.panes.len() > 1).then(|| (split.panes[0].app.clone(), split.panes[1].app.clone()))
    })
    .flatten() else {
        return;
    };
    let gone = if at == 0 {
        let id = second.model.peek().session.clone();
        if !id.is_empty() {
            main.open_session(Open::Resume(id));
        }
        main.window.showing.set(second.window.showing.peek());
        with(|split| split.shape.swap(0, 1));
        1
    } else {
        at
    };
    let Some(pane) = with(|split| {
        (gone < split.panes.len()).then(|| {
            split.shape.remove(gone);
            split.zoom = None;
            split.panes.remove(gone)
        })
    })
    .flatten() else {
        return;
    };
    drop(pane.app.cmd.send(Cmd::Detach));
    changed();
    let last = with(|split| split.panes.len() - 1).unwrap_or(0);
    focus(at.min(last));
    if let Some(owner) = pane.owner {
        sigil::reactive::defer(move || owner.dispose());
    }
}

fn focus(at: usize) {
    let Some((focus, hidden)) = with(|split| {
        let hidden = split.zoom.is_some_and(|zoomed| zoomed != at);
        if hidden {
            split.zoom = None;
        }
        (split.focus, hidden)
    }) else {
        return;
    };
    focus.set(at);
    if hidden {
        changed();
    }
}

fn changed() {
    if let Some(revision) = with(|split| split.revision) {
        revision.update(|value| *value = value.wrapping_add(1));
    }
}
