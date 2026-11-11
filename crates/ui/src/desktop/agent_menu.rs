use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use sigil::prelude::{NodeBuilder, Signal, Widget, button, container, modal, signal, text_field};
use sigil::widgets::{MenuItem, ModalBuilder};

use super::App;
use super::style::DIALOG;
use summonr_agent::hub::Cmd;
use summonr_agent::hub::Status;
use summonr_agent::session::SessionInfo;

/// What a dialog opened from the agent menu acts on.
#[derive(Clone, PartialEq)]
enum Dialog {
    Rename { id: String, name: Signal<String> },
    RenameSpace { space: String, name: Signal<String> },
    Delete { id: String, title: String },
    Quit { running: usize },
}

thread_local! {
    static DIALOG_OPEN: Cell<Option<(Signal<bool>, Signal<Option<Dialog>>)>> = const { Cell::new(None) };
}

/// Creates the agent menu's dialog state for this window.
pub(super) fn install() {
    DIALOG_OPEN.with(|slot| slot.set(Some((signal(false), signal(None)))));
}

/// Whether a rename or delete dialog is open.
pub(super) fn is_open() -> bool {
    DIALOG_OPEN
        .with(Cell::get)
        .is_some_and(|(open, _)| open.peek())
}

fn show(dialog: Dialog) {
    if let Some((open, current)) = DIALOG_OPEN.with(Cell::get) {
        current.set(Some(dialog));
        open.set(true);
    }
}

/// Asks for a new name for workspace `space`.
pub(super) fn rename_space(space: &str, name: &str) {
    show(Dialog::RenameSpace {
        space: space.to_owned(),
        name: signal(name.to_owned()),
    });
}

/// Asks whether to quit while `running` agents still work.
pub(super) fn ask_quit(running: usize) {
    show(Dialog::Quit { running });
}

/// Marks `id` unread, running or not.
pub(super) fn mark_unread(app: &App, id: &str) {
    drop(app.cmd.send(Cmd::MarkUnread(id.to_owned())));
    super::restore::mark_unread(id);
}

/// Whether this window shows thread `id` in any pane.
fn shown(app: &App, id: &str) -> bool {
    app.model.peek().session == id || super::split::shows(id)
}

fn reveal(info: &SessionInfo) -> Option<MenuItem> {
    let folder = PathBuf::from(info.cwd.clone()?);
    let label = if summonr_agent::worktree::repo_of(&folder).is_some() {
        "Reveal worktree"
    } else {
        "Reveal repository"
    };
    Some(MenuItem::new(label).activate(move || {
        drop(summonr_auth::open::external(&folder));
    }))
}

/// The right-click menu of one agent row.
pub(super) fn items(app: &App, info: &SessionInfo, status: Option<Status>) -> Vec<MenuItem> {
    let (id, title) = (info.id.clone(), super::sidebar::title(info));
    let shown = shown(app, &id);
    let running = matches!(status, Some(Status::Working | Status::NeedsYou));
    let mut items = vec![MenuItem::new("Rename…").activate({
        let (id, title) = (id.clone(), title.clone());
        move || {
            show(Dialog::Rename {
                id: id.clone(),
                name: signal(title.clone()),
            })
        }
    })];
    items.extend(split_item(&id));
    let pin = id.clone();
    let word = if super::shelf::is_pinned(&id) {
        "Unpin"
    } else {
        "Pin"
    };
    items.push(MenuItem::new(word).activate(move || super::shelf::flip_pin(&pin)));
    if matches!(status, None | Some(Status::Idle)) {
        let (mark, unread) = (app.clone(), id.clone());
        items.push(
            MenuItem::new("Mark unread")
                .hint("⌘⇧U")
                .activate(move || mark_unread(&mark, &unread)),
        );
    }
    items.extend(space_items(info));
    items.extend(reveal(info));
    if running {
        let (stop, target) = (app.clone(), id.clone());
        items.push(
            MenuItem::new("Stop")
                .hint("⌘.")
                .activate(move || drop(stop.cmd.send(Cmd::Stop(target.clone())))),
        );
    }
    if !shown && !running {
        let shelve = id.clone();
        items.push(MenuItem::new("Archive").activate(move || super::shelf::archive(&shelve)));
        items.push(MenuItem::new("Delete…").activate(move || {
            show(Dialog::Delete {
                id: id.clone(),
                title: title.clone(),
            });
        }));
    }
    items
}

fn split_item(id: &str) -> Option<MenuItem> {
    let id = id.to_owned();
    if !super::split::shows(&id) {
        return Some(MenuItem::new("Open in split").activate(move || {
            super::split::beside(&id);
        }));
    }
    super::split::multiple()
        .then(|| MenuItem::new("Close split").activate(move || super::split::close_chat(&id)))
}

/// The right-click menu of one repository row.
pub(super) fn folder_items(app: &App, folder: &std::path::Path) -> Vec<MenuItem> {
    let (here, beside, reveal) = (
        folder.to_path_buf(),
        folder.to_path_buf(),
        folder.to_path_buf(),
    );
    let open = app.clone();
    let gone = folder.to_path_buf();
    let mut items = vec![
        MenuItem::new("New agent here").activate(move || {
            super::split::show(&open, summonr_agent::hub::Open::Folder(here.clone()));
        }),
        MenuItem::new("New agent in split").activate(move || {
            super::split::beside_in(beside.clone());
        }),
        MenuItem::new("Reveal repository").activate(move || {
            drop(summonr_auth::open::external(&reveal));
        }),
    ];
    if super::spaces::is_added(folder) {
        items.push(
            MenuItem::new("Remove from sidebar")
                .activate(move || super::spaces::remove_repository(&gone)),
        );
    }
    items
}

/// The rename or delete dialog, when one is open.
pub(super) fn view(app: &App) -> Option<NodeBuilder> {
    let (open, current) = DIALOG_OPEN.with(Cell::get)?;
    if !open.get() {
        return None;
    }
    let dialog = current.get()?;
    let body = match dialog {
        Dialog::Rename { id, name } => rename_dialog(app, open, &id, name),
        Dialog::RenameSpace { space, name } => space_dialog(open, space, name),
        Dialog::Delete { id, title } => delete_dialog(app, open, id, &title),
        Dialog::Quit { running } => quit_dialog(open, running),
    };
    Some(container().child(body.width(DIALOG)))
}

/// The items that move agent `info` into or out of a workspace.
fn space_items(info: &SessionInfo) -> Vec<MenuItem> {
    let list = super::spaces::all();
    let home = super::spaces::of(&list, info);
    let mut items = Vec::new();
    if let Some(at) = home.filter(|at| list[*at].sessions.contains(&info.id)) {
        let id = info.id.clone();
        items.push(
            MenuItem::new(format!("Remove from {}", list[at].name))
                .activate(move || super::spaces::take_out(&id)),
        );
    }
    for (_, space) in list.iter().enumerate().filter(|(at, _)| Some(*at) != home) {
        let (id, target) = (info.id.clone(), space.id.clone());
        items.push(
            MenuItem::new(format!("Move to {}", space.name))
                .activate(move || super::spaces::put(&id, &target)),
        );
    }
    let id = info.id.clone();
    items.push(
        MenuItem::new("New project with this agent").activate(move || {
            let made = super::spaces::create("New project");
            super::spaces::put(&id, &made);
            rename_space(&made, "New project");
        }),
    );
    items
}

/// What the project name field shows while it is empty.
const PROJECT_NAME: &str = "Project name";

fn space_dialog(open: Signal<bool>, space: String, name: Signal<String>) -> ModalBuilder {
    let close = move || open.set(false);
    let save = Rc::new(move |text: &str| {
        let text = text.trim();
        if !text.is_empty() {
            super::spaces::rename(&space, text);
        }
    });
    let submit = save.clone();
    modal(open)
        .title("Name project")
        .child(
            text_field(name)
                .placeholder(PROJECT_NAME)
                .autofocus()
                .submit(move |text| {
                    submit(text);
                    close();
                }),
        )
        .actions(
            button("Cancel").ghost().hint("esc").activate(close),
            button("Save").primary().hint("⏎").activate(move || {
                save(&name.peek());
                close();
            }),
        )
}

fn rename(app: &App, id: &str, text: &str) {
    let name = text.trim();
    if name.is_empty() {
        return;
    }
    drop(app.cmd.send(Cmd::Rename {
        session: id.to_owned(),
        name: name.to_owned(),
    }));
    drop(app.cmd.send(Cmd::ListSessions));
}

fn rename_dialog(app: &App, open: Signal<bool>, id: &str, name: Signal<String>) -> ModalBuilder {
    let close = move || open.set(false);
    let run = {
        let (app, id) = (app.clone(), id.to_owned());
        move |text: &str| rename(&app, &id, text)
    };
    let save = {
        let run = run.clone();
        move || {
            run(&name.peek());
            close();
        }
    };
    modal(open)
        .title("Rename agent")
        .child(text_field(name).autofocus().submit(move |text| {
            run(text);
            close();
        }))
        .actions(
            button("Cancel").ghost().hint("esc").activate(close),
            button("Rename").primary().hint("⏎").activate(save),
        )
}

fn delete_dialog(app: &App, open: Signal<bool>, id: String, title: &str) -> ModalBuilder {
    let close = move || open.set(false);
    let sender = app.clone();
    modal(open).title(format!("Delete “{title}”?")).actions(
        button("Cancel").ghost().hint("esc").activate(close),
        button("Delete")
            .tone(sigil::prelude::Tone::Error)
            .activate(move || {
                drop(sender.cmd.send(Cmd::DeleteSession(id.clone())));
                drop(sender.cmd.send(Cmd::ListSessions));
                close();
            }),
    )
}

fn quit_dialog(open: Signal<bool>, running: usize) -> ModalBuilder {
    let agents = if running == 1 { "agent" } else { "agents" };
    modal(open)
        .title(format!("Quit and stop {running} running {agents}?"))
        .actions(
            button("Keep running")
                .ghost()
                .hint("esc")
                .activate(move || open.set(false)),
            button("Quit")
                .tone(sigil::prelude::Tone::Error)
                .activate(super::quit::leave),
        )
}
