use sigil::Host;
use sigil::events::{
    EventRouter, KeyEvent, KeyEventInit, KeyPhase, LogicalKey, ModifiersState, PhysicalKey,
    ShellEvent,
};
use sigil::reactive::Runtime;
use sigil::terminal::Screen;
use tokio::sync::mpsc::unbounded_channel;

use super::super::app::App;
use super::super::view::root;
use super::busy_info;
use crate::tui::{Activity, Msg};

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

fn pending_after_n(draft: &str) -> bool {
    Runtime::scope("approval-keys", || {
        let workspace = tempfile::tempdir().unwrap();
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.resize(100, 30);
        app.update(Msg::Busy(Some(Activity::Turn)));
        app.update(Msg::Event(crate::Event::Approval {
            detail: Some("rm -rf build".to_owned()),
            persist: true,
        }));
        app.replace(draft.to_owned());
        let mut host = Host::cells();
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(100, 30);
        let mut router = EventRouter::new();
        for _ in 0..2 {
            screen.frame(&mut host, &mut router, [], (100, 30)).unwrap();
        }
        screen
            .frame(&mut host, &mut router, [press("n")], (100, 30))
            .unwrap();
        app.approval.peek().is_some()
    })
}

#[test]
fn a_plain_key_answers_an_approval_only_from_an_empty_composer() {
    assert!(!pending_after_n(""));
    assert!(pending_after_n("dont"));
}

fn after_key(draft: &str, key: &str, check: impl FnOnce(&App) -> bool) -> bool {
    Runtime::scope("composer-keys", || {
        let workspace = tempfile::tempdir().unwrap();
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.resize(100, 30);
        app.replace(draft.to_owned());
        let mut host = Host::cells();
        let tree = app.clone();
        host.mount(move || root(tree));
        let mut screen = Screen::new(100, 30);
        let mut router = EventRouter::new();
        for _ in 0..2 {
            screen.frame(&mut host, &mut router, [], (100, 30)).unwrap();
        }
        screen
            .frame(&mut host, &mut router, [press(key)], (100, 30))
            .unwrap();
        check(&app)
    })
}

#[test]
fn a_question_mark_opens_help_only_from_an_empty_composer() {
    assert!(after_key("", "?", |app| app.help.open.peek()));
    assert!(!after_key("why", "?", |app| app.help.open.peek()));
}

fn approval_screen(persist: bool) -> String {
    super::frame_with(busy_info(), 100, 30, super::State::Busy, |app| {
        app.update(Msg::Event(crate::Event::Approval {
            detail: Some("rm -rf build".to_owned()),
            persist,
        }));
    })
    .join("\n")
}

#[test]
fn the_approval_band_lists_its_answer_keys() {
    let screen = approval_screen(true);
    assert!(screen.contains("Needs approval: rm -rf build"), "{screen}");
    assert!(screen.contains("y allow · Y always · n deny"), "{screen}");
    assert!(
        screen.contains("Allow") && screen.contains("Always") && screen.contains("Deny"),
        "{screen}"
    );
    let screen = approval_screen(false);
    assert!(screen.contains("y allow · n deny"), "{screen}");
    assert!(!screen.contains("Y always"), "{screen}");
}

#[test]
fn a_long_single_line_paste_folds_as_one_line() {
    let folded = Runtime::scope("paste-fold", || {
        let workspace = tempfile::tempdir().unwrap();
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        assert!(app.paste_text(&"x".repeat(1200)));
        app.draft.peek()
    });
    assert!(folded.contains("[pasted 1 line #1]"), "{folded}");
}
