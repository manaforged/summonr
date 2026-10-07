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

fn seat(app: &App, levels: &[Effort]) {
    let mut snapshot = app.model.peek().snapshot.clone();
    snapshot.levels = levels.to_vec();
    app.update(Msg::Settings(snapshot));
}

#[test]
fn slash_effort_opens_the_strip_with_the_model_levels() {
    let screen = screen(|app| {
        seat(
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
        seat(
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
fn slash_effort_skips_levels_the_model_lacks() {
    let screen = screen(|app| {
        seat(app, &[Effort::Low, Effort::Medium, Effort::High]);
        run(app, "/effort xhigh");
    });
    assert!(screen.contains("stub-model (high)"), "{screen}");
}

#[test]
fn slash_effort_with_a_level_sets_it_without_the_strip() {
    let screen = screen(|app| {
        seat(
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
        seat(app, &[Effort::Minimal, Effort::Low]);
        app.replace("/effort ".to_owned());
        seat(app, &[Effort::XHigh]);
        app.replace("/effort ".to_owned());
    })
    .join("\n");
    assert!(shown.contains("1 / 1"), "{shown}");
    assert!(shown.contains("xhigh"), "{shown}");
}

#[test]
fn a_model_without_effort_levels_shows_a_note() {
    let screen = screen(|app| {
        seat(app, &[]);
        run(app, "/effort");
    });
    assert!(
        screen.contains("stub-model takes no effort level"),
        "{screen}"
    );
}
