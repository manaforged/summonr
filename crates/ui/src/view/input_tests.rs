use sigil::prelude::HostMode;
use std::cell::RefCell;
use std::rc::Rc;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use sigil::Host;
use sigil::clipboard::{Clipboard, Delivery};
use sigil::events::Event;
use sigil::prelude::NodeBuilder;
use sigil::prelude::{Widget, button};
use sigil::terminal::{Screen, map_key};
use tokio::sync::mpsc::unbounded_channel;

use crate::app::App;
use crate::pane::Pane;
use crate::view::root;

struct MemoryClipboard(Rc<RefCell<String>>);

impl Clipboard for MemoryClipboard {
    fn set(&self, text: &str) -> Result<Delivery, sigil::Error> {
        self.0.replace(text.to_owned());
        Ok(Delivery::Confirmed)
    }

    fn get(&self) -> Option<String> {
        Some(self.0.borrow().clone())
    }
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    map_key(&KeyEvent::new(code, modifiers)).expect("map editor key")
}

fn drive_cmds<T>(
    setup: impl FnOnce(&App),
    view: impl FnOnce(App) -> NodeBuilder + 'static,
    keys: Vec<Event>,
    read: impl FnOnce(&App) -> T,
) -> (T, String, Vec<summonr_agent::hub::Cmd>) {
    sigil::reactive::Owner::scope("input-shortcuts", || {
        let copied = Rc::new(RefCell::new(String::new()));
        sigil::clipboard::install(MemoryClipboard(Rc::clone(&copied)));
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, mut receiver) = unbounded_channel();
        let app = App::new(
            crate::profile::info(),
            workspace.path().to_owned(),
            commands,
        );
        app.resize(160, 40);
        setup(&app);
        let mut host = Host::new(HostMode::Cell);
        let tree = app.clone();
        host.mount(move || view(tree));
        let mut screen = Screen::new(160, 40);
        for _ in 0..2 {
            screen.frame(&mut host, [], (160, 40)).expect("focus input");
        }
        for event in keys {
            screen
                .frame(&mut host, [event], (160, 40))
                .expect("route shortcut");
        }
        let text = copied.borrow().clone();
        let mut sent = Vec::new();
        while let Ok(command) = receiver.try_recv() {
            sent.push(command);
        }
        (read(&app), text, sent)
    })
}

fn drive<T>(
    setup: impl FnOnce(&App),
    view: impl FnOnce(App) -> NodeBuilder + 'static,
    keys: Vec<Event>,
    read: impl FnOnce(&App) -> T,
) -> (T, String) {
    let (seen, text, _) = drive_cmds(setup, view, keys, read);
    (seen, text)
}

#[cfg(not(target_os = "macos"))]
#[test]
fn cutting_file_text_uses_the_file_selection_with_an_empty_composer() {
    use summonr_agent::hub::{Fact, Msg};

    let seen = drive(
        |app| {
            app.view_open("notes.txt".to_owned());
            app.update(Msg::Fact(Fact::View {
                root: None,
                path: "notes.txt".to_owned(),
                file: "notes.txt".into(),
                text: "file text".to_owned(),
                added: Vec::new(),
                image: None,
            }));
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

#[test]
fn capture_dispatch_preserves_leader_binding_composer_and_hotkey_priority() {
    use summonr_agent::hub::{Cmd, Fact, Msg};

    let (armed_and_editing, copied, armed_cmds) = drive_cmds(
        |app| {
            app.view_open("notes.txt".to_owned());
            app.update(Msg::Fact(Fact::View {
                root: None,
                path: "notes.txt".to_owned(),
                file: "notes.txt".into(),
                text: "body".to_owned(),
                added: Vec::new(),
                image: None,
            }));
            app.edit_start();
        },
        root,
        vec![
            key(KeyCode::Char('x'), KeyModifiers::CONTROL),
            key(KeyCode::Char('s'), KeyModifiers::CONTROL),
        ],
        |app| (app.hotkeys.armed.peek(), app.view_edit.peek()),
    );
    assert_eq!((armed_and_editing, copied), ((false, true), String::new()));
    assert!(!armed_cmds.iter().any(|cmd| matches!(cmd, Cmd::Save(..))));

    let (editing, _, save_cmds) = drive_cmds(
        |app| {
            app.view_open("notes.txt".to_owned());
            app.update(Msg::Fact(Fact::View {
                root: None,
                path: "notes.txt".to_owned(),
                file: "notes.txt".into(),
                text: "body".to_owned(),
                added: Vec::new(),
                image: None,
            }));
            app.edit_start();
        },
        root,
        vec![key(KeyCode::Char('s'), KeyModifiers::CONTROL)],
        |app| app.view_edit.peek(),
    );
    assert!(editing);
    assert!(save_cmds.iter().any(|cmd| {
        matches!(cmd, Cmd::Save(path, text) if path == std::path::Path::new("notes.txt") && text == "body")
    }));

    let edge_over_composer = drive(
        |app| app.replace("hello".to_owned()),
        root,
        vec![
            key(KeyCode::Home, KeyModifiers::CONTROL),
            key(KeyCode::Char('!'), KeyModifiers::NONE),
        ],
        |app| app.draft.peek(),
    );
    assert_eq!(edge_over_composer.0, "hello!");

    let newline_hotkey = drive(
        |app| app.replace("ab".to_owned()),
        root,
        vec![key(KeyCode::Char('j'), KeyModifiers::CONTROL)],
        |app| app.draft.peek(),
    );
    assert_eq!(newline_hotkey.0, "ab\n");
}
