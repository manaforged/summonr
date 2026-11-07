use std::cell::Cell;

use sigil::icons::Icon;
use sigil::prelude::{NodeBuilder, Signal, Widget, command_menu, container, modal, signal};

use super::settings::Page;
use super::{App, Pane};
use summonr_agent::hub::{Open, Status};
use summonr_agent::prefs::Action;

use super::style::DIALOG;

const GUTTER: f32 = 48.0;

/// Whether the palette shows, and whether it lists the actions to pin instead of everything.
#[derive(Clone, Copy)]
struct Shown {
    open: Signal<bool>,
    pinning: Signal<bool>,
}

impl Shown {
    fn close(self) {
        self.open.set(false);
        self.pinning.set(false);
    }
}

thread_local! {
    static OPEN: Cell<Option<Shown>> = const { Cell::new(None) };
}

/// Creates the palette's open flag for this window.
pub(super) fn install() {
    let shown = Shown {
        open: signal(false),
        pinning: signal(false),
    };
    OPEN.with(|open| open.set(Some(shown)));
}

/// Shows the ⌘K palette; false when this window has none.
pub(crate) fn show() -> bool {
    OPEN.with(Cell::get)
        .inspect(|shown| {
            shown.pinning.set(false);
            shown.open.set(true);
        })
        .is_some()
}

/// Shows the palette's list of actions to pin.
pub(super) fn show_pins() {
    if let Some(shown) = OPEN.with(Cell::get) {
        shown.pinning.set(true);
        shown.open.set(true);
    }
}

/// The palette: every agent, searchable by title and repo, then actions with their shortcuts.
pub(super) fn view(app: &App) -> Option<NodeBuilder> {
    let shown = OPEN.with(Cell::get)?;
    if !shown.open.get() {
        return None;
    }
    let menu = if shown.pinning.get() {
        pin_list(app, shown)
    } else {
        everything(app, shown)
    };
    Some(container().child(modal(shown.open).child(menu).width(DIALOG + GUTTER)))
}

fn everything(app: &App, shown: Shown) -> NodeBuilder {
    let open = shown.open;
    let live = app.model.get().live.clone();
    let menu = app
        .sessions
        .all_chats()
        .into_iter()
        .fold(command_menu(), |menu, info| {
            let repo = info
                .cwd
                .as_deref()
                .map(|cwd| super::sidebar::folder_name(std::path::Path::new(cwd)));
            let needs = live
                .iter()
                .any(|entry| entry.session == info.id && entry.status == Status::NeedsYou);
            let time = info.modified.map(super::sidebar::row::ago);
            let detail = [needs.then(|| "Needs you".to_string()), repo, time]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
            let (app, id) = (app.clone(), info.id.clone());
            menu.item(super::sidebar::title(&info), move || {
                open.set(false);
                super::shelf::unarchive(&id);
                super::split::show(&app, Open::Resume(id.clone()));
            })
            .detail(detail)
        });
    let menu = app
        .window
        .terminals
        .list
        .get()
        .iter()
        .fold(menu, |menu, term| {
            let (app, id) = (app.clone(), term.id);
            let detail = format!("Terminal · {}", super::sidebar::folder_name(&term.folder));
            menu.item(term.label(), move || {
                open.set(false);
                super::terminal::reveal(&app, id);
            })
            .detail(detail)
        });
    let menu = ROWS.iter().fold(menu, |menu, &(action, label, key, _)| {
        let app = app.clone();
        menu.item(label, move || {
            open.set(false);
            run(&app, action);
        })
        .detail(key)
    });
    let menu = if all_pinned(app) {
        menu
    } else {
        menu.item(PIN, move || shown.pinning.set(true))
    };
    menu.placeholder("Search")
        .cancel(move || shown.close())
        .width(DIALOG)
        .into()
}

/// The item that lists the actions a person can pin.
pub(super) const PIN: &str = "Pin to sidebar…";

/// Whether every action is already on the sidebar.
pub(super) fn all_pinned(app: &App) -> bool {
    app.model
        .with(|model| ROWS.iter().all(|row| model.prefs.sidebar.contains(&row.0)))
}

fn pin_list(app: &App, shown: Shown) -> NodeBuilder {
    let pinned = app.model.with(|model| model.prefs.sidebar.clone());
    ROWS.iter()
        .filter(|row| !pinned.contains(&row.0))
        .fold(command_menu(), |menu, &(action, label, _, _)| {
            let app = app.clone();
            menu.item(label, move || {
                shown.close();
                pin(&app, action);
            })
        })
        .placeholder("Search")
        .cancel(move || shown.close())
        .width(DIALOG)
        .into()
}

/// Adds `action` to the end of the sidebar's rows and saves the list.
pub(super) fn pin(app: &App, action: Action) {
    app.flip(|prefs| {
        if !prefs.sidebar.contains(&action) {
            prefs.sidebar.push(action);
        }
    });
}

/// Takes `action` off the sidebar and saves the list.
pub(super) fn unpin(app: &App, action: Action) {
    app.flip(|prefs| prefs.sidebar.retain(|pinned| *pinned != action));
}

type Row = (Action, &'static str, &'static str, Icon);

const ROWS: [Row; 24] = [
    (Action::NewAgent, "New agent", "⌘N", Icon::Plus),
    (
        Action::NewBranch,
        "New agent on branch",
        "",
        Icon::GitBranch,
    ),
    (
        Action::NewTerminal,
        super::terminal::NEW_TERMINAL_ACTION,
        "⌘T",
        Icon::Terminal,
    ),
    (Action::NewWorkflow, "New workflow", "", Icon::Layers),
    (Action::ClaudeCode, "Claude Code terminal", "", Icon::Code),
    (Action::Split, "Split", "⌘D", Icon::PanelLeft),
    (Action::ClosePane, "Close pane", "⌘W", Icon::CircleX),
    (Action::Needing, "Next agent needing you", "⌘⌥A", Icon::Bell),
    (Action::Search, "Search", "⌘K", Icon::Search),
    (Action::Model, "Change model", "", Icon::Cpu),
    (Action::Changes, "Changes", "⌘⇧D", Icon::FileDiff),
    (Action::Files, "Files", "", Icon::FileText),
    (Action::Worktrees, "Branches", "", Icon::ListTree),
    (Action::Jobs, "Jobs", "", Icon::Activity),
    (Action::Peers, "Peers", "", Icon::Users),
    (Action::Ledger, "Tasks", "", Icon::List),
    (Action::Steps, "All steps", "", Icon::ScrollText),
    (Action::Settings, "Settings", "⌘,", Icon::Settings),
    page(Action::Models, Page::Models),
    page(Action::Workflows, Page::Workflows),
    page(Action::Usage, Page::Usage),
    page(Action::Mcp, Page::Mcp),
    page(Action::Plugins, Page::Plugins),
    page(Action::Shortcuts, Page::Shortcuts),
];

const fn page(action: Action, at: Page) -> Row {
    (action, at.name(), "", at.icon())
}

const PANES: [(Action, Pane); 6] = [
    (Action::Changes, Pane::Diff),
    (Action::Files, Pane::Files),
    (Action::Worktrees, Pane::Worktrees),
    (Action::Jobs, Pane::Jobs),
    (Action::Peers, Pane::Peers),
    (Action::Ledger, Pane::Ledger),
];

const PAGES: [(Action, Page); 6] = [
    (Action::Models, Page::Models),
    (Action::Workflows, Page::Workflows),
    (Action::Usage, Page::Usage),
    (Action::Mcp, Page::Mcp),
    (Action::Plugins, Page::Plugins),
    (Action::Shortcuts, Page::Shortcuts),
];

/// The label, shortcut and icon the palette and the sidebar show for `action`.
pub(super) fn row(action: Action) -> (&'static str, &'static str, Icon) {
    ROWS.iter()
        .find(|row| row.0 == action)
        .map_or(("", "", Icon::Circle), |row| (row.1, row.2, row.3))
}

fn lookup<T: Copy>(table: &[(Action, T)], action: Action) -> Option<T> {
    table.iter().find(|row| row.0 == action).map(|row| row.1)
}

/// Runs `action` in the focused pane.
pub(super) fn run(app: &App, action: Action) {
    use crate::view::keys::bindings::Split;
    let pane = app.target();
    if let Some(page) = lookup(&PAGES, action) {
        return super::settings::open(app, page);
    }
    if let Some(side) = lookup(&PANES, action) {
        return pane.toggle_pane(side);
    }
    match action {
        Action::NewAgent => super::split::show(app, Open::New),
        Action::NewBranch => super::split::show(app, Open::Worktree(pane.workspace())),
        Action::NewTerminal => app.open_terminal(super::terminal::NEW_TERMINAL, None),
        Action::NewWorkflow => super::settings::workflows::create(app),
        Action::ClaudeCode => app.open_terminal(
            super::terminal::CLAUDE,
            Some(super::terminal::CLAUDE_COMMAND),
        ),
        Action::Split => super::split::act(Split::Open),
        Action::ClosePane => super::split::act(Split::Close),
        Action::Needing => super::jump(app, crate::view::keys::bindings::Jump::Needing),
        Action::Search => drop(show()),
        Action::Model => pane.begin_model_edit(),
        Action::Steps => all_steps(&pane),
        Action::Settings => app.settings_open.set(true),
        Action::Changes
        | Action::Files
        | Action::Worktrees
        | Action::Jobs
        | Action::Peers
        | Action::Ledger
        | Action::Models
        | Action::Workflows
        | Action::Usage
        | Action::Mcp
        | Action::Plugins
        | Action::Shortcuts => {}
    }
}

fn all_steps(pane: &App) {
    use summonr_agent::prefs::ToolOutput;
    let full = pane.model.peek().prefs.tools == ToolOutput::Full;
    pane.set_tools(if full {
        ToolOutput::Compact
    } else {
        ToolOutput::Full
    });
}
