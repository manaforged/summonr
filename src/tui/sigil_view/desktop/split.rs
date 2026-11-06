use std::cell::RefCell;

use sigil::prelude::{LayoutStyle, NodeBuilder, Paint, Position, Signal, Widget, container, signal};
use sigil::reactive::{Owner, effect, untrack};
use tokio::sync::mpsc::unbounded_channel;

use super::App;
use super::style::{DIVIDER, FOCUS_RING, PANES, RING};
use crate::tui::Cmd;
use crate::tui::hub::{Open, View};
use crate::tui::sigil_view::view::Split as Act;

/// One pane: its agent's view and the owner that holds the pane's effects.
struct Pane {
    app: App,
    owner: Option<Owner>,
}

/// Panes per row; a third and fourth pane start a second row.
const COLUMNS: usize = 2;

struct Split {
    panes: Vec<Pane>,
    focus: Signal<usize>,
    count: Signal<usize>,
    root: Option<Owner>,
}

thread_local! {
    static SPLIT: RefCell<Option<Split>> = const { RefCell::new(None) };
}

/// Makes the window's first pane from `main`.
pub(super) fn install(main: &App) {
    let split = Split {
        panes: vec![Pane { app: main.clone(), owner: None }],
        focus: signal(0),
        count: signal(1),
        root: Owner::current(),
    };
    SPLIT.with(|slot| *slot.borrow_mut() = Some(split));
}

fn with<R>(f: impl FnOnce(&mut Split) -> R) -> Option<R> {
    SPLIT.with(|slot| slot.borrow_mut().as_mut().map(f))
}

/// The pane that takes keys, when the window is split and it is not the first.
pub(in crate::tui::sigil_view) fn focused() -> Option<App> {
    with(|split| {
        let at = split.focus.peek();
        (at > 0).then(|| split.panes.get(at).map(|pane| pane.app.clone())).flatten()
    })
    .flatten()
}

/// The focused pane's app, or `main` when the window is not split; tracked.
pub(super) fn current(main: &App) -> App {
    let Some((focus, count)) = with(|split| (split.focus, split.count)) else {
        return main.clone();
    };
    let (at, _) = (focus.get(), count.get());
    with(|split| split.panes.get(at).map(|pane| pane.app.clone())).flatten().unwrap_or_else(|| main.clone())
}

#[cfg(test)]
pub(super) fn apps() -> Vec<App> {
    with(|split| split.panes.iter().map(|pane| pane.app.clone()).collect()).unwrap_or_default()
}

/// Whether any pane shows the agent `id`.
pub(super) fn shows(id: &str) -> bool {
    with(|split| split.panes.iter().any(|pane| pane.app.model.peek().session == id)).unwrap_or(false)
}

/// Whether more than one pane is open; tracked.
pub(super) fn active() -> bool {
    with(|split| split.count).is_some_and(|count| count.get() > 1)
}

/// Whether `app` is the window's first pane.
pub(super) fn is_first(app: &App) -> bool {
    with(|split| split.panes.first().is_some_and(|pane| pane.app.cmd.same_channel(&app.cmd))).unwrap_or(true)
}

/// Runs a split key.
pub(in crate::tui::sigil_view) fn act(action: Act) {
    match action {
        Act::Open => open(),
        Act::Close => close_focused(),
        Act::Focus(next) => step(next),
    }
}

/// Opens `open` in the focused pane, or focuses the pane that already shows that agent.
pub(super) fn show(main: &App, open: Open) {
    if let Open::Resume(id) = &open
        && let Some(at) = with(|split| split.panes.iter().position(|pane| pane.app.model.peek().session == *id)).flatten()
    {
        return focus(at);
    }
    focused().unwrap_or_else(|| main.clone()).open_session(open);
}

fn open() {
    let Some((main, workspace, root)) = with(|split| {
        (split.panes.len() < PANES).then(|| {
            let base = &split.panes[split.focus.peek().min(split.panes.len() - 1)].app;
            (split.panes[0].app.clone(), base.workspace(), split.root)
        })
    })
    .flatten() else {
        return;
    };
    let owner = Owner::new("split-pane", root);
    let app = owner.with(|| attach(&main));
    app.open_session(Open::Folder(workspace));
    let at = with(|split| {
        split.panes.push(Pane { app, owner: Some(owner) });
        split.panes.len() - 1
    });
    if let Some(at) = at {
        recount();
        focus(at);
    }
}

fn attach(main: &App) -> App {
    let (commands, command_rx) = unbounded_channel();
    let (ui, inbox) = unbounded_channel();
    drop(main.cmd.send(Cmd::Attach(View { commands: command_rx, ui })));
    let app = App::new(main.info.clone(), main.workspace(), commands);
    let size = main.viewport;
    let fit = app.clone();
    effect(move || {
        let (cols, rows) = size.get();
        fit.resize(cols, rows);
    });
    let (watch, list) = (app.clone(), main.cmd.clone());
    let session = sigil::reactive::derived(move || watch.model.with(|model| model.session.clone()));
    effect(move || {
        session.with(|_| ());
        drop(list.send(Cmd::ListSessions));
    });
    app.load();
    app.refresh();
    app.animate();
    let receiver = app.clone();
    sigil::tasks::on_message(
        inbox,
        move |msg| {
            untrack(|| {
                receiver.update(msg);
                receiver.refresh();
            });
        },
        || {},
    );
    app
}

fn close_focused() {
    let Some(at) = with(|split| (split.panes.len() > 1).then(|| split.focus.peek())).flatten() else {
        return;
    };
    close(at);
}

/// Closes the pane that shows `app`.
pub(super) fn close_app(app: &App) {
    if let Some(at) = with(|split| split.panes.iter().position(|pane| pane.app.cmd.same_channel(&app.cmd))).flatten() {
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
        1
    } else {
        at
    };
    let Some(pane) = with(|split| (gone < split.panes.len()).then(|| split.panes.remove(gone))).flatten() else {
        return;
    };
    drop(pane.app.cmd.send(Cmd::Detach));
    if let Some(owner) = pane.owner {
        owner.dispose();
    }
    recount();
    let last = with(|split| split.panes.len() - 1).unwrap_or(0);
    focus(at.min(last));
}

fn step(next: bool) {
    let Some((at, count)) = with(|split| (split.focus.peek(), split.panes.len())) else {
        return;
    };
    focus(if next { (at + 1) % count } else { (at + count - 1) % count });
}

fn focus(at: usize) {
    if let Some(focus) = with(|split| split.focus) {
        focus.set(at);
    }
}

fn recount() {
    if let Some((count, len)) = with(|split| (split.count, split.panes.len())) {
        count.set(len);
    }
}

/// The panes in a grid: one, two side by side, or two rows of two.
pub(super) fn view() -> NodeBuilder {
    let count = with(|split| split.count).unwrap_or_else(|| signal(1));
    container()
        .layout(LayoutStyle::flex_column().grow(1.0).fill_height().min_size(0.0, 0.0))
        .children_reactive(move || {
            let n = count.get();
            let apps: Vec<App> = with(|split| split.panes.iter().map(|pane| pane.app.clone()).collect()).unwrap_or_default();
            untrack(|| grid(&apps, n))
        })
}

fn grid(apps: &[App], n: usize) -> Vec<NodeBuilder> {
    if n <= 1 {
        return apps.first().map(|app| super::thread::main_col(app.clone())).into_iter().collect();
    }
    let columns = COLUMNS;
    apps.chunks(columns)
        .enumerate()
        .map(|(row, chunk)| {
            chunk.iter().enumerate().fold(
                container().layout(LayoutStyle::flex_row().grow(1.0).flex_basis(0.0).fill_width().min_size(0.0, 0.0)),
                |line, (column, app)| line.child(cell(app, row * columns + column)),
            )
        })
        .collect()
}

fn cell(app: &App, at: usize) -> NodeBuilder {
    let focus = with(|split| split.focus).unwrap_or_else(|| signal(0));
    container()
        .layout(LayoutStyle::flex_column().grow(1.0).flex_basis(0.0).fill_height().min_size(0.0, 0.0).position(Position::Relative))
        .border_w(1)
        .border_color(DIVIDER)
        .pointer_down(move |_| {
            if focus.peek() != at {
                focus.set(at);
            }
        })
        .child(super::thread::main_col(app.clone()))
        .children_reactive(move || if focus.get() == at { ring() } else { Vec::new() })
}

/// Four thin edges around the focused pane; edges only, so clicks reach the pane.
fn ring() -> Vec<NodeBuilder> {
    let edge = |layout: LayoutStyle| container().layout(layout.position(Position::Absolute)).bg(FOCUS_RING);
    vec![
        edge(LayoutStyle::block().inset_top(0.0).inset_left(0.0).fill_width().height(RING)),
        edge(LayoutStyle::block().inset_bottom(0.0).inset_left(0.0).fill_width().height(RING)),
        edge(LayoutStyle::block().inset_top(0.0).inset_left(0.0).fill_height().width(RING)),
        edge(LayoutStyle::block().inset_top(0.0).inset_right(0.0).fill_height().width(RING)),
    ]
}
