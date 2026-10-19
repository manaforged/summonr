use sigil::kernel::Role;

use super::super::{Pane, rail};
use super::harness::{changed, drive};
use super::in_window;
use crate::tui::hub::{Live, Status};
use crate::tui::{Cmd, Msg};

#[test]
fn the_rail_offers_its_panes_and_new_sessions_as_named_buttons() {
    let seen = drive(|_| {}, Vec::new(), |_| ());
    for pane in [
        "Files",
        "Changes",
        "Worktrees",
        "Jobs",
        "Peers",
        rail::LEDGER,
    ] {
        assert_eq!(
            seen.named(Role::Button, pane).len(),
            1,
            "{pane}: {:?}",
            seen.controls
        );
    }
    assert_eq!(seen.named(Role::Button, rail::NEW_SESSION).len(), 1);
    assert_eq!(seen.named(Role::Button, rail::NEW_WORKTREE).len(), 1);
    assert!(
        !seen.shows(|text| text == rail::NEW_SESSION || text == "Plan"),
        "{}",
        seen.text()
    );
}

#[test]
fn live_sessions_show_their_titles_and_tell_twins_apart() {
    let live = |session: &str, title: &str| Live {
        session: session.to_owned(),
        title: title.to_owned(),
        workspace: std::path::PathBuf::from("/tmp/proj"),
        status: Status::Idle,
        active: false,
    };
    let seen = drive(
        |app| {
            app.update(Msg::Live(vec![
                live("aaaaaaaaaaaaaaaa", "fix tests"),
                live("bbbbbbbbbbbbbbbb", "fix tests"),
                live("cccccccccccccccc", ""),
            ]));
        },
        Vec::new(),
        |_| (),
    );
    let twins: Vec<&String> = seen
        .texts
        .iter()
        .filter(|text| text.starts_with("fix tests"))
        .collect();
    assert_eq!(twins.len(), 2, "{}", seen.text());
    assert_ne!(twins[0], twins[1]);
    assert!(seen.shows(|text| text == rail::UNTITLED), "{}", seen.text());
}

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
            app.update(Msg::Diff(changed("HEAD", &[])));
        },
        Vec::new(),
        |app| {
            app.toggle_session();
            app.update(Msg::Diff(changed("HEAD", &[])));
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
fn window_pane_empty_states_name_no_command_line_flags() {
    if !in_window("window_chrome::window_pane_empty_states_name_no_command_line_flags") {
        return;
    }
    for pane in [Pane::Worktrees, Pane::Jobs, Pane::Peers, Pane::Ledger] {
        let seen = drive(|app| app.toggle_pane(pane), Vec::new(), |_| ());
        assert!(
            !seen.shows(|text| text.contains("--") || text.contains("run_command")),
            "{pane:?}: {}",
            seen.text()
        );
    }
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
fn window_dialogs_have_a_thin_border_and_wide_pages() {
    if !in_window("window_chrome::window_dialogs_have_a_thin_border_and_wide_pages") {
        return;
    }
    use crate::tui::sigil_view::ui::Size;
    let cols = (1440.0 / crate::tui::sigil_view::canvas::cell().w) as u16;
    assert_eq!(crate::tui::sigil_view::canvas::frame_border(), 1);
    assert!(Size::Page.width(cols) > Size::Normal.width(cols));
    assert_eq!(Size::Reading.width(cols), Size::Page.width(cols));
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
            let seat = |id: String| crate::recent::Seat::new("openrouter", &id);
            let names: Vec<String> = (0..40).map(|n| format!("model-{n}")).collect();
            app.update(Msg::Models {
                request: 1,
                rows: names.iter().cloned().map(seat).collect(),
                facts: names
                    .iter()
                    .map(|id| (seat(id.clone()), crate::catalog::Model::named(id)))
                    .collect(),
                groups: std::collections::BTreeMap::new(),
                causes: std::collections::BTreeMap::new(),
                pinned: 0,
                recent: 0,
                loading: false,
                error: String::new(),
            });
        },
        Vec::new(),
        |app| {
            let before = (app.picker.sel.peek(), app.picker.scroll.peek());
            let scrolled = app.overlay_scroll(3);
            (
                before,
                scrolled,
                app.picker.sel.peek(),
                app.picker.scroll.peek(),
            )
        },
    );
    let ((sel, scroll), scrolled, after_sel, after_scroll) = seen.read;
    assert!(scrolled);
    assert_eq!(after_sel, sel);
    assert!(after_scroll > scroll, "{scroll} -> {after_scroll}");
}
