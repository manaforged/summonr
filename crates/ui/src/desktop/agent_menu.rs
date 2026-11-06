use std::cell::Cell;
use std::path::PathBuf;

use sigil::prelude::{NodeBuilder, Signal, Widget, button, container, modal, signal, text_field};
use sigil::widgets::MenuItem;

use super::App;
use super::style::DIALOG;
use summonr_agent::hub::Cmd;
use summonr_agent::hub::Status;
use summonr_agent::session::SessionInfo;

/// What a dialog opened from the agent menu acts on.
#[derive(Clone, PartialEq)]
enum Dialog {
    Rename { id: String, name: Signal<String> },
    Delete { id: String, title: String },
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

/// The right-click menu of one agent row.
pub(super) fn items(app: &App, info: &SessionInfo, status: Option<Status>) -> Vec<MenuItem> {
    let (id, title) = (info.id.clone(), super::sidebar::title(info));
    let current = app.model.peek().session == id;
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
    if let Some(folder) = info.cwd.clone().map(PathBuf::from) {
        let reveal = if summonr_agent::worktree::repo_of(&folder).is_some() {
            "Reveal worktree"
        } else {
            "Reveal repository"
        };
        items.push(MenuItem::new(reveal).activate(move || {
            drop(summonr_auth::open::external(&folder));
        }));
    }
    if running {
        let (stop, target) = (app.clone(), id.clone());
        items.push(
            MenuItem::new("Stop")
                .shortcut("⌘.")
                .activate(move || drop(stop.cmd.send(Cmd::Stop(target.clone())))),
        );
    }
    if !current && !running {
        items.push(MenuItem::new("Delete…").activate(move || {
            show(Dialog::Delete {
                id: id.clone(),
                title: title.clone(),
            });
        }));
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
    let close = move || open.set(false);
    let body = match dialog {
        Dialog::Rename { id, name } => {
            let run = {
                let (app, id) = (app.clone(), id.clone());
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
                    button("Cancel").ghost().shortcut("esc").activate(close),
                    button("Rename").primary().shortcut("⏎").activate(save),
                )
        }
        Dialog::Delete { id, title } => {
            let sender = app.clone();
            modal(open).title(format!("Delete “{title}”?")).actions(
                button("Cancel").ghost().shortcut("esc").activate(close),
                button("Delete")
                    .tone(sigil::prelude::Tone::Error)
                    .activate(move || {
                        drop(sender.cmd.send(Cmd::DeleteSession(id.clone())));
                        drop(sender.cmd.send(Cmd::ListSessions));
                        close();
                    }),
            )
        }
    };
    Some(container().child(body.width(DIALOG)))
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
