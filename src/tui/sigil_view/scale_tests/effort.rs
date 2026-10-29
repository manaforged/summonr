use super::super::app::App;
use super::{State, busy_info, frame_with};
use crate::provider::Effort;
use crate::tui::Msg;

fn screen(setup: impl FnOnce(&App)) -> String {
    frame_with(busy_info(), 100, 24, State::Idle, setup).join("\n")
}

fn run(app: &App, line: &str) {
    app.replace(line.to_owned());
    app.submit();
}

fn selection(app: &App, levels: &[Effort]) {
    let mut snapshot = app.model.peek().snapshot.clone();
    snapshot.levels = levels.to_vec();
    app.update(Msg::Settings(snapshot));
}

#[test]
fn slash_effort_opens_the_strip_with_the_model_levels() {
    let screen = screen(|app| {
        selection(
            app,
            &[Effort::Low, Effort::Medium, Effort::High, Effort::XHigh],
        );
        run(app, "/effort");
    });
    let row = screen
        .lines()
        .find(|line| line.contains("low") && line.contains("xhigh"))
        .unwrap_or_else(|| panic!("no effort strip:\n{screen}"));
    assert!(!row.contains("max"), "model caps at xhigh: {row}");
    assert!(row.contains("now off"), "{row}");
}

#[test]
fn enter_on_the_effort_strip_sets_the_chosen_level() {
    let screen = screen(|app| {
        selection(
            app,
            &[Effort::Low, Effort::Medium, Effort::High, Effort::XHigh],
        );
        run(app, "/effort");
        app.overlay_adjust(1);
        app.overlay_adjust(1);
        app.overlay_adjust(1);
        app.overlay_enter();
    });
    assert!(screen.contains("stub-model (xhigh)"), "{screen}");
}

#[test]
fn effort_keyboard_navigation_repaints_the_highlight_without_a_model_update() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use sigil::Host;
    use sigil::events::EventRouter;
    use sigil::reactive::Runtime;
    use sigil::terminal::Screen;
    use tokio::sync::mpsc::unbounded_channel;

    Runtime::scope("effort-keyboard", || {
        let workspace = tempfile::tempdir().expect("workspace");
        let (commands, _receiver) = unbounded_channel();
        let app = App::new(busy_info(), workspace.path().to_owned(), commands);
        app.resize(100, 24);
        selection(&app, &[Effort::Low, Effort::Medium, Effort::High]);
        run(&app, "/effort low");
        run(&app, "/effort");
        let mut host = Host::cells();
        host.mount(move || super::super::view::root(app));
        let mut screen = Screen::new(100, 24);
        let mut router = EventRouter::new();
        for _ in 0..2 {
            screen
                .frame(&mut host, &mut router, [], (100, 24))
                .expect("open effort strip");
        }
        for (code, modifiers, expected) in [
            (KeyCode::Right, KeyModifiers::NONE, "medium"),
            (KeyCode::Tab, KeyModifiers::NONE, "high"),
            (KeyCode::BackTab, KeyModifiers::SHIFT, "medium"),
            (KeyCode::Left, KeyModifiers::NONE, "low"),
            (KeyCode::Down, KeyModifiers::NONE, "medium"),
            (KeyCode::Up, KeyModifiers::NONE, "low"),
        ] {
            let key = sigil::terminal::map_key(&KeyEvent::new(code, modifiers)).expect("key");
            screen
                .frame(&mut host, &mut router, [key], (100, 24))
                .expect("navigate effort strip");
            let grid = screen
                .frame(&mut host, &mut router, [], (100, 24))
                .expect("paint selected effort");
            let text = grid.text();
            let (y, row) = text
                .lines()
                .enumerate()
                .find(|(_, line)| line.contains("effort") && line.contains("now low"))
                .expect("effort strip");
            let x = row[..row.find(expected).expect("effort level")]
                .chars()
                .count();
            assert!(
                grid.get(x, y).expect("effort cell").bold,
                "{code:?} must highlight {expected} without a model update"
            );
        }
    });
}

#[test]
fn slash_effort_skips_levels_the_model_lacks() {
    let screen = screen(|app| {
        selection(app, &[Effort::Low, Effort::Medium, Effort::High]);
        run(app, "/effort xhigh");
    });
    assert!(screen.contains("stub-model (high)"), "{screen}");
}

#[test]
fn slash_effort_with_a_level_sets_it_without_the_strip() {
    let screen = screen(|app| {
        selection(
            app,
            &[Effort::Low, Effort::Medium, Effort::High, Effort::Max],
        );
        run(app, "/effort high");
    });
    assert!(
        !screen.lines().any(|line| line.contains("now off")),
        "{screen}"
    );
    assert!(screen.contains("stub-model (high)"), "{screen}");
}

#[test]
fn effort_suggestions_follow_the_seated_model() {
    let shown = frame_with(busy_info(), 110, 30, State::Idle, |app| {
        selection(app, &[Effort::Minimal, Effort::Low]);
        app.replace("/effort ".to_owned());
        selection(app, &[Effort::XHigh]);
        app.replace("/effort ".to_owned());
    })
    .join("\n");
    assert!(shown.contains("1 / 1"), "{shown}");
    assert!(shown.contains("xhigh"), "{shown}");
}

#[test]
fn a_model_without_effort_levels_shows_a_note() {
    let screen = screen(|app| {
        selection(app, &[]);
        run(app, "/effort");
    });
    assert!(
        screen.contains("stub-model takes no effort level"),
        "{screen}"
    );
}
