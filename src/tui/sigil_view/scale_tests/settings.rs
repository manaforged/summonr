use super::{State, busy_info, frame_with};

#[test]
fn the_settings_dialog_names_itself_and_the_harness_group() {
    let shown = frame_with(busy_info(), 110, 40, State::Idle, |app| {
        app.replace("/settings".to_owned());
        app.submit();
    })
    .join("\n");
    assert!(
        shown.contains("Settings") && shown.contains("harness"),
        "{shown}"
    );
    assert!(
        !shown.contains("Options") && !shown.contains(" agent "),
        "{shown}"
    );
}

#[test]
fn the_theme_row_cycles_space_nord_dawn_and_auto() {
    let mut seen = Vec::new();
    let _ = frame_with(busy_info(), 110, 40, State::Idle, |app| {
        for _ in 0..4 {
            app.toggle_theme();
            seen.push(app.model.peek().prefs.theme.clone());
        }
    });
    assert_eq!(seen, ["nord", "dawn", "auto", "space"]);
}

#[test]
fn the_notify_row_shows_the_choice_and_cycles_it() {
    let mut seen = Vec::new();
    let mut label = String::new();
    let _ = frame_with(busy_info(), 110, 40, State::Idle, |app| {
        label = super::super::settings::setting_rows(app)
            .into_iter()
            .find(|row| row.label == "notify")
            .map(|row| row.value)
            .unwrap_or_default();
        for _ in 0..3 {
            app.flip(|prefs| prefs.notify = prefs.notify.toggle());
            seen.push(app.model.peek().prefs.notify.name());
        }
    });
    assert_eq!(label, "when a turn ends");
    assert_eq!(seen, ["all", "off", "turn"]);
}

#[test]
fn the_window_title_drops_control_characters_from_the_workspace() {
    use super::super::app::App;
    sigil::reactive::Runtime::scope("title-controls", || {
        let (commands, _sent) = tokio::sync::mpsc::unbounded_channel();
        let app = App::new(
            busy_info(),
            std::path::PathBuf::from("/tmp/odd\u{7}\u{1b}]0;x"),
            commands,
        );
        let title = super::super::host::run::title(&app, false);
        assert!(!title.chars().any(char::is_control), "{title:?}");
    });
}

#[test]
fn the_turn_notification_names_how_the_turn_ended() {
    use crate::tui::Msg;
    let mut seen = Vec::new();
    let _ = frame_with(busy_info(), 110, 30, State::Idle, |app| {
        seen.push(super::super::host::run::ending(app));
        app.update(Msg::User("next".to_owned()));
        app.update(Msg::Failed("provider timed out".to_owned()));
        seen.push(super::super::host::run::ending(app));
    });
    assert_eq!(seen, ["turn finished", "turn failed"]);
}
