use super::super::app::App;
use super::{State, busy_info, frame_with};

fn screen(setup: impl FnOnce(&App)) -> String {
    frame_with(busy_info(), 100, 24, State::Idle, setup).join("\n")
}

fn run(app: &App, line: &str) {
    app.replace(line.to_owned());
    app.submit();
}

#[test]
fn slash_effort_opens_the_effort_slider() {
    let screen = screen(|app| run(app, "/effort"));
    assert!(
        screen.contains("reasoning effort for stub-model"),
        "{screen}"
    );
    assert!(
        screen
            .lines()
            .any(|line| line.contains("none") && line.contains("medium") && line.contains("high")),
        "{screen}"
    );
}

#[test]
fn enter_on_the_effort_slider_sets_the_chosen_level() {
    let screen = screen(|app| {
        run(app, "/effort");
        app.overlay_adjust(1);
        app.overlay_adjust(1);
        app.overlay_enter();
    });
    assert!(!screen.contains("reasoning effort for"), "{screen}");
    assert!(screen.contains("stub-model (medium)"), "{screen}");
}

#[test]
fn slash_effort_with_a_level_sets_it_without_the_slider() {
    let screen = screen(|app| run(app, "/effort high"));
    assert!(!screen.contains("reasoning effort for"), "{screen}");
    assert!(screen.contains("stub-model (high)"), "{screen}");
}
