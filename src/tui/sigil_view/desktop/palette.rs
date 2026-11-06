use std::cell::Cell;

use sigil::prelude::{NodeBuilder, Signal, Widget, command_menu, container, modal, signal};

use super::{App, Pane};
use crate::tui::hub::{Open, Status};

use super::style::DIALOG;

const GUTTER: f32 = 48.0;

thread_local! {
    static OPEN: Cell<Option<Signal<bool>>> = const { Cell::new(None) };
}

/// Creates the palette's open flag for this window.
pub(super) fn install() {
    OPEN.with(|open| open.set(Some(signal(false))));
}

/// Shows the ⌘K palette; false when this window has none.
pub(in crate::tui::sigil_view) fn show() -> bool {
    OPEN.with(Cell::get).inspect(|open| open.set(true)).is_some()
}

/// The palette: every agent, searchable by title and repo, then actions with their shortcuts.
pub(super) fn view(app: &App) -> Option<NodeBuilder> {
    let open = OPEN.with(Cell::get)?;
    if !open.get() {
        return None;
    }
    let live = app.model.get().live.clone();
    let menu = app.sessions.all_chats().into_iter().fold(command_menu(), |menu, info| {
        let repo = info.cwd.as_deref().map(|cwd| super::sidebar::folder_name(std::path::Path::new(cwd)));
        let needs = live.iter().any(|entry| entry.session == info.id && entry.status == Status::NeedsYou);
        let detail = [needs.then(|| "Needs you".to_string()), repo].into_iter().flatten().collect::<Vec<_>>().join(" · ");
        let (app, id) = (app.clone(), info.id.clone());
        menu.item(super::sidebar::title(&info), move || {
            open.set(false);
            super::split::show(&app, Open::Resume(id.clone()));
        })
        .detail(detail)
    });
    let menu = app.terminals.list.get().iter().fold(menu, |menu, term| {
        let (app, id) = (app.clone(), term.id);
        let detail = format!("Terminal · {}", super::sidebar::folder_name(&term.folder));
        menu.item(term.label(), move || {
            open.set(false);
            super::terminal::reveal(&app, id);
        })
        .detail(detail)
    });
    let menu = ACTIONS.iter().fold(menu, |menu, (label, key, action)| {
        let (app, action) = (app.clone(), *action);
        menu.item(*label, move || {
            open.set(false);
            run(&app, action);
        })
        .detail(*key)
    });
    let menu = menu.placeholder("Search").cancel(move || open.set(false)).width(DIALOG);
    Some(container().child(modal(open).child(menu).width(DIALOG + GUTTER)))
}

#[derive(Clone, Copy)]
enum Action {
    NewAgent,
    NewTerminal,
    ClaudeCode,
    Model,
    Changes,
    Files,
    Settings,
    Steps,
    Needing,
    Split,
    ClosePane,
}

const ACTIONS: [(&str, &str, Action); 11] = [
    ("New agent", "⌘N", Action::NewAgent),
    ("New terminal", "⌘T", Action::NewTerminal),
    ("Claude Code", "", Action::ClaudeCode),
    ("Split", "⌘D", Action::Split),
    ("Close pane", "⌘W", Action::ClosePane),
    ("Next agent needing you", "⌘⌥A", Action::Needing),
    ("Change model", "", Action::Model),
    ("Changes", "⌘⇧D", Action::Changes),
    ("Files", "", Action::Files),
    ("All steps", "", Action::Steps),
    ("Settings", "⌘,", Action::Settings),
];

fn run(app: &App, action: Action) {
    use crate::tui::sigil_view::view::Split;
    let pane = app.target();
    match action {
        Action::NewAgent => super::split::show(app, Open::New),
        Action::NewTerminal => app.open_terminal(super::terminal::NEW_TERMINAL, None),
        Action::ClaudeCode => app.open_terminal(super::terminal::CLAUDE, Some(super::terminal::CLAUDE_COMMAND)),
        Action::Split => super::split::act(Split::Open),
        Action::ClosePane => super::split::act(Split::Close),
        Action::Model => pane.begin_model_edit(),
        Action::Changes => pane.toggle_pane(Pane::Diff),
        Action::Files => pane.toggle_pane(Pane::Files),
        Action::Settings => app.settings_open.set(true),
        Action::Needing => super::jump(app, crate::tui::sigil_view::view::Jump::Needing),
        Action::Steps => {
            use crate::prefs::ToolOutput;
            let full = pane.model.peek().prefs.tools == ToolOutput::Full;
            pane.set_tools(if full { ToolOutput::Compact } else { ToolOutput::Full });
        }
    }
}
