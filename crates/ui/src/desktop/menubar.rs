use sigil::app::menu::{Menu, StandardItem};
use sigil::events::{LogicalKey, Shortcut};

use super::{App, Pane};
use crate::view::keys::bindings::{Jump, KeyCommand, Split, WINDOW};
use summonr_agent::prefs::Hotkey;

const SETTINGS: &str = "Settings…";
const QUIT: &str = "Quit Summonr";

const FILE: [(&str, KeyCommand); 4] = [
    ("New agent", KeyCommand::NewSession),
    ("New terminal", KeyCommand::Terminal),
    ("Split pane", KeyCommand::Split(Split::Open)),
    ("Close pane", KeyCommand::Split(Split::Close)),
];
const VIEW: [(&str, KeyCommand); 4] = [
    ("Search", KeyCommand::Palette),
    ("Sidebar", KeyCommand::Sidebar),
    ("Changes", KeyCommand::Pane(Pane::Diff)),
    ("Terminal", KeyCommand::Pane(Pane::Terminal)),
];
const AGENT: [(&str, KeyCommand); 2] = [
    ("Stop", KeyCommand::Interrupt),
    ("Next that needs you", KeyCommand::Jump(Jump::Needing)),
];
const SETTINGS_KEY: KeyCommand = KeyCommand::Hotkey(Hotkey::Options);

/// Installs the menu bar and runs what the person picks in the focused pane.
pub(super) fn install(app: &App) {
    let main = app.clone();
    let picks = Menu::events().subscribe(move |id: String| run(&main, &id));
    sigil::reactive::on_cleanup(move || drop(picks));
    if let Err(error) = bar().install() {
        app.notice.set(Some(crate::app::Notice {
            text: format!("Could not add the menu bar: {error}"),
            kind: crate::app::NoticeKind::Failed,
        }));
    }
}

fn run(main: &App, id: &str) {
    if id == QUIT {
        return super::quit::request();
    }
    let command = if id == SETTINGS {
        Some(SETTINGS_KEY)
    } else {
        FILE.iter()
            .chain(&VIEW)
            .chain(&AGENT)
            .find(|(label, _)| *label == id)
            .map(|(_, command)| *command)
    };
    if let Some(command) = command {
        super::split::current(main).run_key(command);
    }
}

fn shortcut(command: KeyCommand) -> Option<Shortcut> {
    WINDOW
        .iter()
        .find(|(_, _, bound)| *bound == command)
        .map(|(key, modifiers, _)| Shortcut::new(LogicalKey::character(*key), *modifiers))
}

fn items(rows: &[(&'static str, KeyCommand)]) -> Menu {
    rows.iter().fold(Menu::new(), |menu, (label, command)| {
        menu.item(*label, *label, shortcut(*command))
    })
}

fn bar() -> Menu {
    let summonr = Menu::new()
        .standard(StandardItem::About)
        .separator()
        .item(SETTINGS, SETTINGS, shortcut(SETTINGS_KEY))
        .separator()
        .standard(StandardItem::Hide)
        .standard(StandardItem::HideOthers)
        .standard(StandardItem::ShowAll)
        .separator()
        .item(
            QUIT,
            QUIT,
            Some(Shortcut::primary(LogicalKey::character("q"))),
        );
    let edit = Menu::new()
        .standard(StandardItem::Undo)
        .standard(StandardItem::Redo)
        .separator()
        .standard(StandardItem::Cut)
        .standard(StandardItem::Copy)
        .standard(StandardItem::Paste)
        .standard(StandardItem::SelectAll);
    let window = Menu::new()
        .standard(StandardItem::Minimize)
        .standard(StandardItem::Maximize)
        .separator()
        .standard(StandardItem::BringAllToFront);
    Menu::new()
        .submenu("Summonr", summonr)
        .submenu("File", items(&FILE))
        .submenu("Edit", edit)
        .submenu("View", items(&VIEW))
        .submenu("Agent", items(&AGENT))
        .submenu("Window", window)
}
