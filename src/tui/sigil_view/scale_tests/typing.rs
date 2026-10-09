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
