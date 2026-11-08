use std::cell::Cell;
use std::path::{Path, PathBuf};

use sigil::app::dialog::Picked;
use sigil::prelude::{Signal, effect, signal};
use summonr_agent::hub::Open;
use summonr_agent::prefs::{Layout, Space};
use summonr_agent::session::SessionInfo;

use super::App;

/// The workspaces, the repositories a person added, the workspace a new agent joins next, and a folder being picked for one.
#[derive(Clone, Copy)]
struct Spaces {
    list: Signal<Vec<Space>>,
    repos: Signal<Vec<PathBuf>>,
    joining: Signal<Option<String>>,
    picking: Signal<Option<(String, Picked<PathBuf>)>>,
}

thread_local! {
    static SPACES: Cell<Option<Spaces>> = const { Cell::new(None) };
}

/// Creates the workspaces at the window's root and puts each new agent in the workspace that started it.
pub(super) fn install(app: &App) {
    let made = Spaces {
        list: signal(Vec::new()),
        repos: signal(Vec::new()),
        joining: signal(None),
        picking: signal(None),
    };
    SPACES.with(|slot| slot.set(Some(made)));
    effect(move || {
        let Some((space, picked)) = made.picking.get() else {
            return;
        };
        if let sigil::tasks::Status::Ok(found) = picked.get() {
            sigil::reactive::defer(move || {
                made.picking.set(None);
                if let Some(folder) = found {
                    add_folder(&space, folder);
                }
            });
        }
    });
    let watch = app.clone();
    effect(move || {
        let id = super::split::current(&watch).model.with(|model| model.session.clone());
        if id.is_empty() {
            return;
        }
        if let Some(space) = sigil::reactive::untrack(|| made.joining.get()) {
            made.joining.set(None);
            put(&id, &space);
        }
    });
}

fn spaces() -> Spaces {
    SPACES
        .with(Cell::get)
        .expect("invariant: the window installs workspaces before it builds a view")
}

/// Takes the workspaces and repositories from a saved layout.
pub(super) fn load(saved: &Layout) {
    spaces().list.set(saved.spaces.clone());
    spaces().repos.set(saved.repositories.clone());
}

/// The repositories a person added, in the order added; tracked.
pub(super) fn repositories() -> Vec<PathBuf> {
    spaces().repos.get()
}

/// Adds `folder` to the repositories, so it lists before any agent runs in it.
pub(super) fn add_repository(folder: PathBuf) {
    let folder = folder.canonicalize().unwrap_or(folder);
    spaces().repos.update(|list| {
        if !list.contains(&folder) {
            list.push(folder);
        }
    });
}

/// Whether `folder` is one of the repositories a person added.
pub(super) fn is_added(folder: &Path) -> bool {
    spaces().repos.peek().iter().any(|seen| seen == folder)
}

/// Takes `folder` off the repositories; its agents still list while they exist.
pub(super) fn remove_repository(folder: &Path) {
    spaces().repos.update(|list| list.retain(|seen| seen != folder));
}

/// The workspaces to save, holding only agents that still exist when the list of agents is known.
pub(super) fn kept(chats: &[String]) -> Vec<Space> {
    let mut list = spaces().list.peek();
    if !chats.is_empty() {
        for space in &mut list {
            space.sessions.retain(|id| chats.contains(id));
        }
    }
    list
}

/// Every workspace, oldest first; tracked.
pub(super) fn all() -> Vec<Space> {
    spaces().list.get()
}

/// Makes an empty workspace named `name` and returns its id.
pub(super) fn create(name: &str) -> String {
    let made = Space::new(name);
    let id = made.id.clone();
    spaces().list.update(|list| list.push(made));
    id
}

fn edit(space: &str, change: impl FnOnce(&mut Space)) {
    spaces().list.update(|list| {
        if let Some(found) = list.iter_mut().find(|seen| seen.id == space) {
            change(found);
        }
    });
}

/// Renames workspace `space`.
pub(super) fn rename(space: &str, name: &str) {
    edit(space, |found| name.clone_into(&mut found.name));
}

/// Removes workspace `space`; its agents go back to their repositories.
pub(super) fn remove(space: &str) {
    spaces().list.update(|list| list.retain(|seen| seen.id != space));
}

/// Asks for a folder and adds it to workspace `space`.
pub(super) fn pick_folder(space: &str) {
    spaces().picking.set(Some((space.to_owned(), super::welcome::folder())));
}

fn add_folder(space: &str, folder: PathBuf) {
    let folder = folder.canonicalize().unwrap_or(folder);
    edit(space, |found| {
        if !found.folders.contains(&folder) {
            found.folders.push(folder);
        }
    });
}

/// Puts agent `id` in workspace `space` and takes it out of any other.
pub(super) fn put(id: &str, space: &str) {
    take_out(id);
    edit(space, |found| found.sessions.push(id.to_owned()));
}

/// Takes agent `id` out of its workspace.
pub(super) fn take_out(id: &str) {
    spaces().list.update(|list| {
        for space in list.iter_mut() {
            space.sessions.retain(|seen| seen != id);
        }
    });
}

/// Starts a new agent in workspace `space`, in its first folder or the home folder, and puts it there.
pub(super) fn new_agent(app: &App, space: &str) {
    let folder = spaces()
        .list
        .peek()
        .iter()
        .find(|seen| seen.id == space)
        .and_then(|found| found.folders.first().cloned())
        .or_else(summonr_core::dirs::home_dir);
    let Some(folder) = folder else {
        return;
    };
    spaces().joining.set(Some(space.to_owned()));
    super::split::show(app, Open::Folder(folder));
}

/// The workspace `info` is in: the one it was put in, else the one holding the deepest folder it ran in.
pub(super) fn of(list: &[Space], info: &SessionInfo) -> Option<usize> {
    if let Some(at) = list.iter().position(|space| space.sessions.contains(&info.id)) {
        return Some(at);
    }
    let cwd = Path::new(info.cwd.as_deref()?);
    list.iter()
        .enumerate()
        .filter_map(|(at, space)| {
            space
                .folders
                .iter()
                .filter(|folder| cwd.starts_with(folder))
                .map(|folder| folder.as_os_str().len())
                .max()
                .map(|depth| (at, depth))
        })
        .max_by_key(|(_, depth)| *depth)
        .map(|(at, _)| at)
}
