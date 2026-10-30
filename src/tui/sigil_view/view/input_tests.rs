use std::cell::RefCell;
use std::rc::Rc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sigil::Host;
use sigil::clipboard::{ClipError, Clipboard, Delivery};
use sigil::events::{EventRouter, ShellEvent};
use sigil::kernel::NodeBuilder;
use sigil::prelude::{NodeEventsExt, button};
use sigil::reactive::Runtime;
use sigil::terminal::{Screen, map_key};
use tokio::sync::mpsc::unbounded_channel;

use crate::tui::sigil_view::app::App;
use crate::tui::sigil_view::pane::Pane;
use crate::tui::sigil_view::view::root;

struct MemoryClipboard(Rc<RefCell<String>>);

impl Clipboard for MemoryClipboard {
    fn set(&self, text: &str) -> Result<Delivery, ClipError> {
        self.0.replace(text.to_owned());
        Ok(Delivery::Confirmed)
    }

    fn get(&self) -> Option<String> {
        Some(self.0.borrow().clone())
    }
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> ShellEvent {
    map_key(&KeyEvent::new(code, modifiers)).expect("map editor key")
}

fn drive<T>(
    setup: impl FnOnce(&App),
    view: impl FnOnce(App) -> NodeBuilder + 'static,
    keys: Vec<ShellEvent>,
    read: impl FnOnce(&App) -> T,
) -> (T, String) {
    Runtime::scope("input-shortcuts", || {
        let copied = Rc::new(RefCell::new(String::new()));
        sigil::clipboard::install(MemoryClipboard(Rc::clone(&copied)));
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(
            crate::tui::sigil_view::profile::info(),
            workspace.path().to_owned(),
            commands,
        );
        app.resize(160, 40);
        setup(&app);
        let mut host = Host::cells();
        let tree = app.clone();
        host.mount(move || view(tree));
        let mut screen = Screen::new(160, 40);
        let mut router = EventRouter::new();
        for _ in 0..2 {
            screen
                .frame(&mut host, &mut router, [], (160, 40))
                .expect("focus input");
        }
        for event in keys {
            screen
                .frame(&mut host, &mut router, [event], (160, 40))
                .expect("route shortcut");
        }
        let text = copied.borrow().clone();
        (read(&app), text)
    })
}

#[cfg(not(target_os = "macos"))]
#[test]
fn cutting_file_text_uses_the_file_selection_with_an_empty_composer() {
    use crate::tui::Msg;

    let seen = drive(
        |app| {
            app.view_open("notes.txt".to_owned());
            app.update(Msg::View {
                path: "notes.txt".to_owned(),
                file: "notes.txt".into(),
                text: "file text".to_owned(),
                added: Vec::new(),
                image: None,
            });
            app.edit_start();
        },
        root,
        vec![
            key(KeyCode::End, KeyModifiers::SHIFT),
            key(KeyCode::Char('x'), KeyModifiers::CONTROL),
        ],
        |app| (app.view_text.peek(), app.draft.peek()),
    );
    assert_eq!(
        seen,
        ((String::new(), String::new()), "file text".to_owned())
    );
}

#[cfg(not(target_os = "macos"))]
#[test]
fn cutting_filter_text_uses_the_filter_selection_with_an_empty_composer() {
    let seen = drive(
        |app| {
            app.begin_model_edit();
            app.replace_model_draft("needle".to_owned());
        },
        root,
        vec![
            key(KeyCode::Home, KeyModifiers::SHIFT),
            key(KeyCode::Char('x'), KeyModifiers::CONTROL),
        ],
        |app| (app.picker.list.query.peek(), app.draft.peek()),
    );
    assert_eq!(seen, ((String::new(), String::new()), "needle".to_owned()));
}

#[cfg(not(target_os = "macos"))]
#[test]
fn select_all_replaces_the_composer_instead_of_inserting_at_the_start() {
    let seen = drive(
        |app| app.replace("replace me".to_owned()),
        root,
        vec![
            key(KeyCode::Char('a'), KeyModifiers::CONTROL),
            key(KeyCode::Char('Z'), KeyModifiers::NONE),
        ],
        |app| app.draft.peek(),
    );
    assert_eq!(seen.0, "Z");
}

#[test]
fn the_editor_prefix_opens_files_without_cutting_the_draft() {
    let seen = drive(
        |app| app.replace("keep".to_owned()),
        root,
        vec![
            key(KeyCode::Char('x'), KeyModifiers::CONTROL),
            key(KeyCode::Char('e'), KeyModifiers::NONE),
        ],
        |app| (app.layout.peek().secondary, app.draft.peek()),
    );
    assert_eq!(
        seen,
        ((Some(Pane::Files), "keep".to_owned()), String::new())
    );
}

#[test]
fn the_prefix_opens_files_when_a_button_has_focus() {
    let seen = drive(
        |_| {},
        |app| super::super::keys::shell(&app).child(button("Action").autofocus()),
        vec![
            key(KeyCode::Char('x'), KeyModifiers::CONTROL),
            key(KeyCode::Char('e'), KeyModifiers::NONE),
        ],
        |app| app.layout.peek().secondary,
    );
    assert_eq!(seen.0, Some(Pane::Files));
}

#[cfg(target_os = "macos")]
#[test]
fn the_mac_editor_prefix_preserves_selected_text() {
    let seen = drive(
        |app| {
            app.edit("keep");
            app.editor.select(0, true);
        },
        root,
        vec![
            key(KeyCode::Char('x'), KeyModifiers::CONTROL),
            key(KeyCode::Char('e'), KeyModifiers::NONE),
        ],
        |app| (app.layout.peek().secondary, app.draft.peek()),
    );
    assert_eq!(
        seen,
        ((Some(Pane::Files), "keep".to_owned()), String::new())
    );
}
