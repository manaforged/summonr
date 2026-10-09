use sigil::Host;
use sigil::events::{
    EventRouter, KeyEvent, KeyEventInit, KeyPhase, LogicalKey, ModifiersState, PhysicalKey,
    ShellEvent,
};
use sigil::reactive::Runtime;
use sigil::terminal::Screen;
use tokio::sync::mpsc::unbounded_channel;

use super::super::app::{App, Overlay};
use super::super::view::root;
use super::busy_info;

fn press(key: &str) -> ShellEvent {
    ShellEvent::Key(KeyEvent::new(KeyEventInit {
        key: LogicalKey::character(key),
        physical: PhysicalKey::Unidentified,
        modifiers: ModifiersState::empty(),
        repeat: false,
        phase: KeyPhase::Down,
        text: Some(key.into()),
    }))
}

fn arrow(key: sigil::prelude::Key) -> ShellEvent {
    ShellEvent::Key(KeyEvent::new(KeyEventInit {
        key: LogicalKey::named(key),
        physical: PhysicalKey::Unidentified,
        modifiers: ModifiersState::empty(),
        repeat: false,
        phase: KeyPhase::Down,
        text: None,
    }))
}

fn typed(command: &str, read: impl FnOnce(&App) -> String) -> String {
    after(command, vec![press("k"), press("i")], read)
}

fn after<T>(command: &str, keys: Vec<ShellEvent>, read: impl FnOnce(&App) -> T) -> T {
    Runtime::scope("dialog-typing", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.resize(160, 40);
        let mut host = Host::cells();
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(160, 40);
        let mut router = EventRouter::new();
        for _ in 0..2 {
            screen
                .frame(&mut host, &mut router, [], (160, 40))
                .expect("frame");
        }
        app.replace(command.to_owned());
        app.submit();
        for _ in 0..2 {
            screen
                .frame(&mut host, &mut router, [], (160, 40))
                .expect("frame");
        }
        screen
            .frame(&mut host, &mut router, keys, (160, 40))
            .expect("frame");
        read(&app)
    })
}

#[test]
fn shift_enter_keeps_a_multiline_draft_until_plain_enter_submits_it() {
    use crate::tui::Cmd;
    use crossterm::event::{KeyCode, KeyEvent as TerminalKey, KeyModifiers};

    Runtime::scope("composer-line-break", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, mut sent) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        assert!(matches!(sent.try_recv(), Ok(Cmd::Offers)));
        app.resize(160, 40);
        app.update(crate::tui::Msg::User("Earlier message".to_owned()));
        app.replace("first".to_owned());
        let mut host = Host::cells();
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(160, 40);
        let mut router = EventRouter::new();
        for _ in 0..2 {
            screen
                .frame(&mut host, &mut router, [], (160, 40))
                .expect("focus the composer");
        }
        let events =
            [
                sigil::terminal::map_key(&TerminalKey::new(KeyCode::Enter, KeyModifiers::SHIFT))
                    .expect("map Shift+Enter"),
            ];
        screen
            .frame(&mut host, &mut router, events, (160, 40))
            .expect("insert the line break");
        assert_eq!(app.draft.peek(), "first\n");
        assert_eq!(app.caret.peek(), 6);
        assert!(sent.try_recv().is_err(), "Shift+Enter submitted the draft");
        screen
            .frame(&mut host, &mut router, [press("second")], (160, 40))
            .expect("type the second line");
        assert_eq!(app.draft.peek(), "first\nsecond");
        screen
            .frame(
                &mut host,
                &mut router,
                [sigil::terminal::key_down(sigil::prelude::Key::Enter)],
                (160, 40),
            )
            .expect("submit with plain Enter");
        let Cmd::Submit(input) = sent.try_recv().expect("submitted input") else {
            panic!("plain Enter did not submit the draft");
        };
        assert_eq!(input.text, "first\nsecond");
        assert!(app.draft.peek().is_empty());
    });
}

#[test]
fn alt_enter_line_break_keeps_the_draft_unsent() {
    use crossterm::event::{KeyCode, KeyEvent as TerminalKey, KeyModifiers};

    Runtime::scope("composer-alt-line-break", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, mut sent) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        drop(sent.try_recv());
        app.resize(160, 40);
        app.replace("first".to_owned());
        let mut host = Host::cells();
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(160, 40);
        let mut router = EventRouter::new();
        for _ in 0..2 {
            screen
                .frame(&mut host, &mut router, [], (160, 40))
                .expect("focus the composer");
        }
        let events =
            [
                sigil::terminal::map_key(&TerminalKey::new(KeyCode::Enter, KeyModifiers::ALT))
                    .expect("map Alt+Enter"),
            ];
        screen
            .frame(&mut host, &mut router, events, (160, 40))
            .expect("insert the line break");
        assert_eq!(app.draft.peek(), "first\n");
        assert!(sent.try_recv().is_err(), "Alt+Enter submitted the draft");
    });
}

#[test]
fn typing_after_slash_model_searches_the_models() {
    assert_eq!(typed("/model", |app| app.picker.draft.peek()), "ki");
}

#[test]
fn typing_after_slash_login_searches_the_providers() {
    assert_eq!(typed("/login", |app| app.auth.filter.peek()), "ki");
}

#[test]
fn plain_arrows_switch_between_the_models_providers_and_usage_tabs() {
    use sigil::prelude::Key;
    let seen = |keys| after("/model", keys, |app: &App| app.overlay());
    assert_eq!(seen(vec![arrow(Key::ArrowRight)]), Some(Overlay::Auth));
    assert_eq!(
        seen(vec![arrow(Key::ArrowRight), arrow(Key::ArrowRight)]),
        Some(Overlay::Usage)
    );
    assert_eq!(seen(vec![arrow(Key::ArrowLeft)]), Some(Overlay::Usage));
}
