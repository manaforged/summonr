use super::super::Pane;
use super::harness::{changed, drive};
use super::in_window;
use crate::tui::{Cmd, Fact, Msg};

#[test]
fn the_window_slash_list_describes_built_in_commands() {
    if !in_window("window_chrome::the_window_slash_list_describes_built_in_commands") {
        return;
    }
    let settings = crate::command_specs()
        .iter()
        .find(|spec| spec.name.ends_with("settings"))
        .expect("settings command");
    let seen = drive(|app| app.replace("/sett".to_owned()), Vec::new(), |_| ());
    assert!(
        seen.shows(|text| text.contains(settings.description)),
        "{}",
        seen.text()
    );
}

#[test]
fn the_changes_pane_stays_open_and_can_scope_to_this_session() {
    let seen = drive(
        |app| {
            app.toggle_pane(Pane::Diff);
            app.update(Msg::Fact(Fact::Diff(changed("HEAD", &[]))));
        },
        Vec::new(),
        |app| {
            app.toggle_session();
            app.update(Msg::Fact(Fact::Diff(changed("HEAD", &[]))));
            app.layout.peek().secondary
        },
    );
    assert_eq!(seen.read, Some(Pane::Diff));
    let asked = seen
        .sent
        .iter()
        .filter(|cmd| matches!(cmd, Cmd::SessionDiff(_)))
        .count();
    assert_eq!(asked, 2);
}

#[test]
fn a_test_summary_shows_the_line_with_the_most_tests() {
    let text = "test result: ok. 12 passed; 0 failed; 0 ignored\ntest result: ok. 0 passed; 0 failed; 0 ignored";
    assert_eq!(
        crate::tools::command::test_result(text),
        Some("test result: ok. 12 passed; 0 failed; 0 ignored")
    );
}

#[test]
fn window_hint_bars_show_two_keys() {
    if !in_window("window_chrome::window_hint_bars_show_two_keys") {
        return;
    }
    use crate::tui::sigil_view::ui::{Hints, Key};
    let text = Hints::new()
        .key(Key::Enter, "use")
        .key(Key::Esc, "close")
        .key(Key::Tab, "filter")
        .key(Key::UpDown, "move")
        .text(&crate::prefs::Hotkeys::default());
    assert_eq!(text.matches(" · ").count(), 1, "{text}");
}

#[test]
fn the_picker_wheel_scrolls_without_moving_the_selection() {
    if !in_window("window_chrome::the_picker_wheel_scrolls_without_moving_the_selection") {
        return;
    }
    let seen = drive(
        |app| {
            app.begin_model_edit();
            let selection = |id: String| crate::recent::ModelId::new("openrouter", &id);
            let names: Vec<String> = (0..40).map(|n| format!("model-{n}")).collect();
            app.update(Msg::Fact(Fact::Models {
                request: 1,
                rows: names.iter().cloned().map(selection).collect(),
                facts: names
                    .iter()
                    .map(|id| (selection(id.clone()), crate::catalog::Model::named(id)))
                    .collect(),
                groups: std::collections::BTreeMap::new(),
                causes: std::collections::BTreeMap::new(),
                pinned: 0,
                recent: 0,
                loading: false,
                error: String::new(),
            }));
        },
        Vec::new(),
        |app| {
            let before = (app.picker.list.cursor.peek(), app.picker.list.scroll.peek());
            let scrolled = app.overlay_scroll(3);
            (
                before,
                scrolled,
                app.picker.list.cursor.peek(),
                app.picker.list.scroll.peek(),
            )
        },
    );
    let ((sel, scroll), scrolled, after_sel, after_scroll) = seen.read;
    assert!(scrolled);
    assert_eq!(after_sel, sel);
    assert!(after_scroll > scroll, "{scroll} -> {after_scroll}");
}
