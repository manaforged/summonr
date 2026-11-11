use sigil::prelude::{
    AlignItems, NodeBuilder, Paint, Signal, Space, Tone, Widget, effect, scroll, sidebar,
};

use super::App;
use super::style::{RECESSED, S, SCROLLBAR, XS};
use summonr_agent::hub::Cmd;
use summonr_agent::hub::{Live, Status};
use summonr_agent::session::SessionInfo;
use summonr_agent::worktree::Tree;

mod jump;
mod repo;
pub(super) mod row;
mod space;

pub(super) use jump::jump;
use repo::{Group, Seen, repository};

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
    let shut = sigil::prelude::signal(Vec::<std::path::PathBuf>::new());
    let folded = sigil::prelude::signal(Vec::<String>::new());
    let picked = super::panel::picked(&app);
    let on_panel = sigil::reactive::derived(move || picked.with(Option::is_some));
    let middle = app.clone();
    sigil::prelude::row()
        .gap(sigil::prelude::Space::None)
        .align_items(sigil::prelude::AlignItems::Stretch)
        .fill_height()
        .shrink(0.0)
        .child(super::rail::view(&app))
        .child(
            sidebar(
                sigil::prelude::column()
                    .fill()
                    .gap(XS)
                    .min_width(0.0)
                    .min_height(0.0)
                    .child(brand(&app))
                    .child(
                        sigil::prelude::column()
                            .fill_width()
                            .grow(1.0)
                            .gap(XS)
                            .min_width(0.0)
                            .min_height(0.0)
                            .children_reactive(move || {
                                if on_panel.get() {
                                    vec![
                                        scroll(super::panel::side(&middle, picked))
                                            .background(RECESSED)
                                            .grow(1.0)
                                            .into(),
                                    ]
                                } else {
                                    threads(&middle, (shut, folded), (live, shown, actions))
                                }
                            }),
                    )
                    .child(row::footer(&app)),
            )
            .background(RECESSED)
            .width(WIDTH),
        )
}

type Watched = (
    Signal<Vec<Live>>,
    Signal<Vec<String>>,
    Signal<Vec<summonr_agent::prefs::Action>>,
);

/// The pinned actions, the pinned agents, and every repository with its agents.
fn threads(
    app: &App,
    open: (Signal<Vec<std::path::PathBuf>>, Signal<Vec<String>>),
    (live, shown, actions): Watched,
) -> Vec<NodeBuilder> {
    let list = app.clone();
    vec![
        sigil::widgets::context_menu(
            sigil::prelude::column()
                .gap(sigil::prelude::Space::None)
                .fill_width()
                .padding_right(SCROLLBAR)
                .children_reactive({
                    let app = app.clone();
                    move || row::actions(&app, actions)
                }),
            {
                let app = app.clone();
                move || row::add_items(&app)
            },
        )
        .into(),
        sigil::prelude::column()
            .gap(sigil::prelude::Space::None)
            .fill_width()
            .padding_right(SCROLLBAR)
            .children_reactive({
                let app = app.clone();
                move || row::pinned(&app, live, shown)
            }),
        space::heading(app),
        scroll(
            sigil::prelude::column()
                .gap(sigil::prelude::Space::None)
                .fill_width()
                .children_reactive(move || chats(&list, open, live, shown)),
        )
        .background(RECESSED)
        .grow(1.0)
        .into(),
    ]
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
    sigil::prelude::row()
        .fill_width()
        .height(BAR - 16.0)
        .padding_left(Space::raw(
            super::CONTROLS - 8.0,
            "lines the brand up after the window controls",
        ))
        .padding_right(S)
        .gap(S)
        .shrink(0.0)
        .align_items(AlignItems::Center)
        .child(super::sidebar_toggle(app))
        .child(super::drag_area())
}

/// Each workspace with its repositories, then every repository in no workspace; tracked.
fn chats(
    app: &App,
    (shut, folded): (Signal<Vec<std::path::PathBuf>>, Signal<Vec<String>>),
    live: Signal<Vec<Live>>,
    shown: Signal<Vec<String>>,
) -> Vec<NodeBuilder> {
    let seen = Seen {
        live: live.get(),
        shown: shown.get(),
        terms: app.window.terminals.list.get(),
        trees: app.worktrees(),
        closed: shut.get(),
        shut,
    };
    let list = super::spaces::all();
    let (put, rest) = space::split(&list, row::listed(app));
    let mut added = super::spaces::repositories();
    added.extend(list.iter().flat_map(|space| space.folders.iter().cloned()));
    let mut groups = projects(&seen.trees, &app.workspace(), (&added, rest));
    let open_folders = seen
        .terms
        .iter()
        .map(|term| repo_folder(&seen.trees, &term.folder))
        .chain(
            seen.live
                .iter()
                .map(|entry| repo_folder(&seen.trees, &entry.workspace)),
        );
    for folder in open_folders {
        if !groups.iter().any(|(at, _)| *at == folder) {
            groups.push((folder, Vec::new()));
        }
    }
    let (mut nodes, loose) = space::sections(app, (&list, folded), (put, groups), &seen);
    for group in loose {
        nodes.extend(repository(app, group, &seen, 0));
    }
    nodes
}

pub(super) fn topics(
    app: &App,
    agents: Vec<SessionInfo>,
    (shown, live): (&[String], &[Live]),
    depth: u16,
) -> Vec<NodeBuilder> {
    super::topic::group(agents)
        .iter()
        .filter_map(|topic| {
            let selected = topic.threads.iter().any(|info| shown.contains(&info.id));
            super::topic::row(app, topic, (selected, depth), live)
        })
        .collect()
}

/// Every repository an agent ran in, by name, without `here`.
pub(super) fn folders(app: &App) -> Vec<std::path::PathBuf> {
    let here = app.workspace();
    let trees = app.worktrees();
    projects(
        &trees,
        &here,
        (&super::spaces::repositories(), app.sessions.all_chats()),
    )
    .into_iter()
    .map(|(folder, _)| folder)
    .filter(|folder| *folder != repo_folder(&trees, &here))
    .collect()
}

/// The folder an agent lists under: its repository's checkout when it works in a worktree.
fn repo_folder(trees: &[Tree], folder: impl AsRef<std::path::Path>) -> std::path::PathBuf {
    let folder = folder.as_ref();
    trees
        .iter()
        .find(|tree| tree.path == folder || tree.primary == folder)
        .map(|tree| tree.primary.clone())
        .or_else(|| summonr_agent::worktree::checkout_of(folder))
        .unwrap_or_else(|| folder.to_path_buf())
}

/// Every repository with its agents: the current one, the ones a person added, and the ones agents ran in.
fn projects(
    trees: &[Tree],
    here: &std::path::Path,
    (added, rows): (&[std::path::PathBuf], Vec<SessionInfo>),
) -> Vec<(std::path::PathBuf, Vec<SessionInfo>)> {
    let mut groups: Vec<(std::path::PathBuf, Vec<SessionInfo>)> =
        vec![(repo_folder(trees, here), Vec::new())];
    for folder in added {
        let folder = repo_folder(trees, folder);
        if !groups.iter().any(|(seen, _)| *seen == folder) {
            groups.push((folder, Vec::new()));
        }
    }
    for info in rows {
        let folder = info
            .cwd
            .as_deref()
            .map_or_else(|| here.to_path_buf(), |cwd| repo_folder(trees, cwd));
        match groups.iter_mut().find(|(seen, _)| *seen == folder) {
            Some((_, chats)) => chats.push(info),
            None => groups.push((folder, vec![info])),
        }
    }
    let here = repo_folder(trees, here);
    groups.retain(|(folder, chats)| !chats.is_empty() || *folder == here || added.contains(folder));
    groups.sort_by_cached_key(|(folder, _)| folder_name(folder).to_lowercase());
    for (_, agents) in &mut groups {
        agents.sort_by(|a, b| b.id.cmp(&a.id));
    }
    groups
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
        Status::NeedsYou => Tone::Warning,
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
