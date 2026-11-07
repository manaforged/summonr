use std::rc::Rc;

use sigil::icons::Icon;
use sigil::prelude::{
    AlignItems, LayoutStyle, ListRowSlots, NodeBuilder, Paint, Signal, Tone, Widget, container,
    effect, list_row, scroll, sidebar,
};
use sigil::widgets::icon;

use super::App;
use super::style::{ICON, M, RECESSED, S, SCROLLBAR, Text, XS, text};
use summonr_agent::hub::Cmd;
use summonr_agent::hub::{Live, Open, Status};
use summonr_agent::session::SessionInfo;

pub(super) mod row;

pub(super) const WIDTH: f32 = 248.0;
pub(super) const BAR: f32 = 40.0;
/// The narrowest window that keeps the sidebar beside the chat.
pub(super) const ROOMY: f32 = 760.0;
const TITLE_CHARS: usize = 60;
const NEW: &str = "New agent";
/// The pinned actions, every repository with its agents, and Settings.
pub(super) fn view(app: App) -> NodeBuilder {
    let live = sigil::reactive::derived({
        let app = app.clone();
        move || app.model.with(|model| model.live.clone())
    });
    let session = sigil::reactive::derived({
        let app = app.clone();
        move || {
            let pane = super::split::current(&app);
            if pane.window.showing.get().is_some() {
                String::new()
            } else {
                pane.model.with(|model| model.session.clone())
            }
        }
    });
    refresh_on_switch(&app, session, live);
    let shown = sigil::reactive::derived(super::split::shown_chats);
    let actions = sigil::reactive::derived({
        let app = app.clone();
        move || app.model.with(|model| model.prefs.sidebar.clone())
    });
    let list = app.clone();
    let shut = sigil::prelude::signal(Vec::<std::path::PathBuf>::new());
    container()
        .layout(LayoutStyle::flex_column().fill_height().shrink(0.0))
        .child(
            sidebar(
                container()
                    .layout(LayoutStyle::flex_column().fill().gap(XS).min_size(0.0, 0.0))
                    .child(brand(&app))
                    .child(
                        container()
                            .layout(
                                LayoutStyle::flex_column()
                                    .fill_width()
                                    .padding_right(SCROLLBAR),
                            )
                            .children_reactive({
                                let app = app.clone();
                                move || row::actions(&app, actions)
                            }),
                    )
                    .child(
                        container()
                            .layout(
                                LayoutStyle::flex_column()
                                    .fill_width()
                                    .padding_right(SCROLLBAR),
                            )
                            .children_reactive({
                                let app = app.clone();
                                move || row::pinned(&app, live, shown)
                            }),
                    )
                    .child(repositories(&app))
                    .child(
                        scroll(
                            container()
                                .layout(LayoutStyle::flex_column().fill_width())
                                .children_reactive(move || chats(&list, shut, live, shown)),
                        )
                        .bg(RECESSED)
                        .grow(1.0),
                    )
                    .child(row::footer(&app)),
            )
            .bg(RECESSED)
            .width(WIDTH),
        )
}

fn refresh_on_switch(app: &App, session: Signal<String>, live: Signal<Vec<Live>>) {
    let watch = app.clone();
    effect(move || {
        session.with(|_| ());
        live.with(|_| ());
        drop(watch.cmd.send(Cmd::ListSessions));
    });
}

fn brand(app: &App) -> NodeBuilder {
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .height(BAR - 2.0 * S)
                .padding_left(super::CONTROLS - S)
                .padding_right(S)
                .gap(S)
                .shrink(0.0)
                .align_items(AlignItems::Center),
        )
        .child(super::sidebar_toggle(app))
        .child(super::drag_area())
}

/// The Repositories heading, with a button that adds one.
fn repositories(app: &App) -> NodeBuilder {
    let picked = sigil::prelude::signal(None);
    super::welcome::follow(app, picked);
    let add = sigil::prelude::icon_button(Icon::Plus, move || {
        picked.set(Some(super::welcome::folder()))
    })
    .accessible(super::thread::ADD_REPOSITORY);
    container()
        .layout(
            LayoutStyle::flex_row()
                .fill_width()
                .padding_top(M)
                .padding_left(S)
                .padding_right(SCROLLBAR)
                .align_items(AlignItems::Center),
        )
        .child(
            text(Text::Heading, REPOSITORIES)
                .layout(LayoutStyle::block().grow(1.0).min_size(0.0, 0.0)),
        )
        .child(super::tip(super::thread::ADD_REPOSITORY, None, add.into()))
}

const REPOSITORIES: &str = "Repositories";

fn chats(
    app: &App,
    shut: Signal<Vec<std::path::PathBuf>>,
    live: Signal<Vec<Live>>,
    shown: Signal<Vec<String>>,
) -> Vec<NodeBuilder> {
    let live = live.get();
    let shown = shown.get();
    let closed = shut.get();
    let terms = app.window.terminals.list.get();
    let mut groups = projects(&app.workspace(), row::listed(app));
    let open_folders = terms
        .iter()
        .map(|term| term.folder.clone())
        .chain(live.iter().map(|entry| repo_folder(&entry.workspace)));
    for folder in open_folders {
        if !groups.iter().any(|(seen, _)| *seen == folder) {
            groups.push((folder, Vec::new()));
        }
    }
    let mut nodes = Vec::new();
    for (folder, agents) in groups {
        let open = !closed.contains(&folder);
        let states: Vec<Status> = agents
            .iter()
            .filter_map(|info| shown_state(&live, &info.id))
            .collect();
        nodes.push(section(app, &folder, open, &states, shut));
        if !open {
            continue;
        }
        nodes.extend(super::terminal::rows(app, &terms, &folder));
        for topic in super::topic::group(agents) {
            let selected = topic.threads.iter().any(|info| shown.contains(&info.id));
            nodes.extend(super::topic::row(app, &topic, selected, &live));
        }
    }
    nodes
}

/// Every repository an agent ran in, by name, without `here`.
pub(super) fn folders(app: &App) -> Vec<std::path::PathBuf> {
    let here = app.workspace();
    projects(&here, app.sessions.all_chats())
        .into_iter()
        .map(|(folder, _)| folder)
        .filter(|folder| *folder != repo_folder(&here))
        .collect()
}

/// The folder an agent lists under: its repository's checkout when it works in a worktree.
fn repo_folder(folder: impl AsRef<std::path::Path>) -> std::path::PathBuf {
    let folder = folder.as_ref();
    summonr_agent::worktree::checkout_of(folder).unwrap_or_else(|| folder.to_path_buf())
}

fn projects(
    here: &std::path::Path,
    rows: Vec<SessionInfo>,
) -> Vec<(std::path::PathBuf, Vec<SessionInfo>)> {
    let mut groups: Vec<(std::path::PathBuf, Vec<SessionInfo>)> =
        vec![(repo_folder(here), Vec::new())];
    for info in rows {
        let folder = info
            .cwd
            .as_deref()
            .map_or_else(|| here.to_path_buf(), repo_folder);
        match groups.iter_mut().find(|(seen, _)| *seen == folder) {
            Some((_, chats)) => chats.push(info),
            None => groups.push((folder, vec![info])),
        }
    }
    let here = repo_folder(here);
    groups.retain(|(folder, chats)| !chats.is_empty() || *folder == here);
    groups.sort_by_cached_key(|(folder, _)| folder_name(folder).to_lowercase());
    for (_, agents) in &mut groups {
        agents.sort_by(|a, b| b.id.cmp(&a.id));
    }
    groups
}

/// A repository row that folds its agents; folded, it shows their strongest status and count.
fn section(
    app: &App,
    folder: &std::path::Path,
    open: bool,
    states: &[Status],
    shut: sigil::prelude::Signal<Vec<std::path::PathBuf>>,
) -> NodeBuilder {
    let flip = folder.to_path_buf();
    let strongest = [
        Status::NeedsYou,
        Status::Failed,
        Status::Working,
        Status::Review,
    ]
    .into_iter()
    .find(|status| states.contains(status));
    let summary = strongest.filter(|_| !open).map(|status| {
        let count = states.iter().filter(|seen| **seen == status).count();
        (count.to_string().into(), tone(status))
    });
    let folder_icon = if open { Icon::FolderOpen } else { Icon::Folder };
    let row = list_row(
        false,
        folder_name(folder),
        ListRowSlots {
            leading: Some(Box::new(icon(folder_icon).px(ICON))),
            badge: summary,
            activate: Some(Rc::new(move || {
                shut.update(|closed| {
                    if let Some(at) = closed.iter().position(|seen| *seen == flip) {
                        closed.remove(at);
                    } else {
                        closed.push(flip.clone());
                    }
                });
            })),
            ..ListRowSlots::default()
        },
    )
    .expanded(sigil::prelude::signal(open));
    let (menu, folder) = (app.clone(), folder.to_path_buf());
    container()
        .layout(LayoutStyle::flex_row().fill_width().padding_top(S))
        .child(sigil::widgets::context_menu(row, move || {
            super::agent_menu::folder_items(&menu, &folder)
        }))
}

pub(super) fn folder_name(folder: &std::path::Path) -> String {
    summonr_agent::worktree::display_name(folder)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| folder.display().to_string())
}

pub(super) fn live_title(info: &mut SessionInfo, live: &[Live]) {
    if let Some(entry) = live
        .iter()
        .find(|entry| entry.session == info.id && !entry.title.is_empty())
    {
        info.name = Some(entry.title.clone());
    }
}

pub(super) fn shown_state(live: &[Live], session: &str) -> Option<Status> {
    state(live, session).or_else(|| super::restore::mark(session))
}

fn state(live: &[Live], session: &str) -> Option<Status> {
    live.iter()
        .find(|entry| entry.session == session)
        .map(|entry| entry.status)
}

const fn tone(status: Status) -> Tone {
    match status {
        Status::NeedsYou => Tone::Warn,
        Status::Failed => Tone::Error,
        Status::Working => Tone::Info,
        Status::Review | Status::Idle => Tone::Neutral,
    }
}

pub(super) fn title(info: &SessionInfo) -> String {
    let raw = info
        .name
        .clone()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| info.preview.lines().next().unwrap_or_default().to_string());
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return NEW.to_string();
    }
    trimmed.chars().take(TITLE_CHARS).collect()
}

pub(super) fn project_name(app: &App) -> String {
    folder_name(&app.workspace())
}

/// Opens the agent `to` names, in sidebar order.
pub(super) fn jump(app: &App, to: crate::view::keys::bindings::Jump) {
    use crate::view::keys::bindings::Jump;
    let model = app.model.peek();
    let session = super::split::current(app).model.peek().session.clone();
    let order: Vec<String> = super::shelf::pinned()
        .into_iter()
        .chain(
            projects(&app.workspace(), row::listed(app))
                .into_iter()
                .flat_map(|(_, agents)| agents.into_iter().map(|info| info.id)),
        )
        .collect();
    let at = order.iter().position(|id| *id == session);
    let target = match to {
        Jump::Nth(n) => order.get(n - 1),
        Jump::Step(forward) => {
            let len = order.len().max(1);
            let next = match (at, forward) {
                (Some(at), true) => (at + 1) % len,
                (Some(at), false) => (at + len - 1) % len,
                (None, _) => 0,
            };
            order.get(next)
        }
        Jump::Needing => {
            let needing = |id: &&String| state(&model.live, id) == Some(Status::NeedsYou);
            let after = at.map_or(0, |at| at + 1);
            order[after.min(order.len())..]
                .iter()
                .find(needing)
                .or_else(|| order.iter().find(needing))
        }
    };
    if let Some(id) = target.filter(|id| **id != session) {
        super::split::show(app, Open::Resume(id.clone()));
    }
}
