use sigil::events::ModifiersState;
use sigil::prelude::{Key, Role};

use super::super::Pane;
use super::harness::{command, drive, graded, named, settings, typed, unconfigured};
use super::in_window;
use crate::mcp::{Field, FieldKind, Form};
use crate::tui::{Fact, Intent, Msg, Snapshot};

#[test]
fn typing_replaces_the_desktop_composer_selection() {
    if !in_window("window_focus::typing_replaces_the_desktop_composer_selection") {
        return;
    }
    let mut keys = vec![named(
        Key::Home,
        ModifiersState::CONTROL | ModifiersState::SHIFT,
    )];
    keys.extend(typed("X"));
    let seen = drive(
        |app| app.replace("replace me".to_owned()),
        keys,
        |app| app.draft.peek(),
    );
    assert_eq!(seen.read, "X");
}

#[test]
fn a_modal_round_trip_keeps_the_composer_selection() {
    if !in_window("window_focus::a_modal_round_trip_keeps_the_composer_selection") {
        return;
    }
    let mut keys = vec![
        named(Key::Home, ModifiersState::CONTROL | ModifiersState::SHIFT),
        command(","),
        named(Key::Escape, ModifiersState::empty()),
    ];
    keys.extend(typed("X"));
    let seen = drive(
        |app| app.replace("replace me".to_owned()),
        keys,
        |app| (app.settings_open.peek(), app.draft.peek()),
    );
    assert_eq!(seen.read, (false, "X".to_owned()));
}

#[test]
fn keyboard_folder_expansion_updates_the_files_pane() {
    if !in_window("window_focus::keyboard_folder_expansion_updates_the_files_pane") {
        return;
    }
    let seen = drive(
        |app| {
            app.update(Msg::Fact(Fact::Files(vec!["src/lib.rs".to_owned()])));
            app.toggle_pane(Pane::Files);
            app.focus_pane(true);
        },
        vec![named(Key::ArrowRight, ModifiersState::empty())],
        |app| app.pane_list(Pane::Files).is_expanded(&"src".to_owned()),
    );
    assert!(seen.read, "the folder expanded");
    assert!(seen.shows(|text| text == "lib.rs"), "{}", seen.text());
}

fn question() -> Form {
    Form {
        server: Some("fixture".to_owned()),
        message: "Which name?".to_owned(),
        fields: vec![Field {
            name: "name".to_owned(),
            title: "Name".to_owned(),
            description: None,
            kind: FieldKind::Text {
                format: None,
                min_length: None,
                max_length: None,
                pattern: None,
            },
            required: true,
            default: None,
        }],
    }
}

#[test]
fn letters_typed_on_a_focused_button_go_to_the_composer() {
    if !in_window("window_focus::letters_typed_on_a_focused_button_go_to_the_composer") {
        return;
    }
    let mut keys = vec![named(Key::Tab, ModifiersState::empty())];
    keys.extend(typed("fix it"));
    let seen = drive(settings(graded()), keys, |app| app.draft.peek());
    assert_eq!(seen.read, "fix it");
}

#[test]
fn command_k_filters_the_palette_and_escape_closes_it() {
    if !in_window("window_focus::command_k_filters_the_palette_and_escape_closes_it") {
        return;
    }
    let mut keys = vec![named(Key::Tab, ModifiersState::empty()), command("k")];
    keys.extend(typed("model"));
    let filtered = drive(settings(graded()), keys, |app| app.draft.peek());
    assert_eq!(filtered.read, "");
    assert_eq!(filtered.named(Role::MenuItem, "Change model").len(), 1, "{:?}", filtered.controls);
    assert!(filtered.named(Role::MenuItem, "Settings").is_empty(), "{:?}", filtered.controls);
    let closed = drive(|_| {}, vec![command("k"), named(Key::Escape, ModifiersState::empty())], |_| ());
    assert!(closed.named(Role::MenuItem, "New agent").is_empty(), "{:?}", closed.controls);
}

#[test]
fn an_opened_pane_leaves_typing_in_the_composer() {
    if !in_window("window_focus::an_opened_pane_leaves_typing_in_the_composer") {
        return;
    }
    let seen = drive(
        |app| app.toggle_pane(Pane::Diff),
        typed("r"),
        |app| (app.pane_focus.peek(), app.draft.peek()),
    );
    assert_eq!(seen.read, (false, "r".to_owned()));
}

#[test]
fn escape_closes_a_pane_before_it_cancels_a_pending_question() {
    let seen = drive(
        |app| {
            app.form_open(Some(question()), 1);
            app.toggle_pane(Pane::Files);
        },
        vec![
            named(Key::Escape, ModifiersState::empty()),
            named(Key::Escape, ModifiersState::empty()),
        ],
        |app| (app.layout.peek().secondary, app.form_stage().is_some()),
    );
    assert_eq!(seen.read, (None, true));
}

#[test]
fn command_shift_d_opens_the_changes_pane() {
    if !in_window("window_focus::command_shift_d_opens_the_changes_pane") {
        return;
    }
    let seen = drive(
        |_| {},
        vec![super::harness::press(sigil::events::LogicalKey::character("d"), ModifiersState::META | ModifiersState::SHIFT, None)],
        |app| app.layout.peek().secondary,
    );
    assert_eq!(seen.read, Some(Pane::Diff));
}

#[test]
fn the_sidebar_names_no_provider_and_the_model_once() {
    let seen = drive(
        settings(Snapshot {
            endpoint: "stub-provider".to_owned(),
            ..graded()
        }),
        Vec::new(),
        |_| (),
    );
    assert!(
        !seen.shows(|text| text == "stub-provider"),
        "{}",
        seen.text()
    );
    let models = seen
        .texts
        .iter()
        .filter(|text| text.starts_with("stub-model"))
        .count();
    assert_eq!(models, 1, "{}", seen.text());
}

#[test]
fn a_failed_send_with_no_provider_adds_one_notice_and_opens_at_the_top() {
    let seen = drive(settings(unconfigured()), Vec::new(), |app| {
        for _ in 0..2 {
            app.open_providers();
            app.auth.list.cursor.set(5);
            app.cancel_auth();
        }
        app.open_providers();
        let notices = app
            .model
            .peek()
            .entries
            .iter()
            .filter(|entry| matches!(entry, crate::tui::sigil_view::model::Entry::System(_)))
            .count();
        (notices, app.auth.list.cursor.peek())
    });
    assert_eq!(seen.read, (1, 0));
}

#[test]
fn a_plain_path_mention_has_no_quotes() {
    assert_eq!(
        crate::tui::sigil_view::suggest::reference("src/a.rs"),
        "@src/a.rs"
    );
    assert_eq!(
        crate::tui::sigil_view::suggest::reference("my file.rs"),
        "@\"my file.rs\""
    );
}

#[test]
fn a_worktree_session_is_named_by_its_repository() {
    let name = crate::worktree::display_name(std::path::Path::new("/work/tally-worktrees/0a1b2c"));
    assert_eq!(name.as_deref(), Some("tally · worktree"));
    let plain = crate::worktree::display_name(std::path::Path::new("/work/tally"));
    assert_eq!(plain.as_deref(), Some("tally"));
}

#[test]
fn reverting_a_file_and_discarding_a_worktree_each_take_two_clicks() {
    let seen = drive(
        |_| {},
        Vec::new(),
        |app| {
            app.file_revert("src/a.rs");
            app.tree_discard_armed(std::path::PathBuf::from("/work/demo-worktrees/0a1b"));
        },
    );
    assert!(!seen.sent.iter().any(|cmd| matches!(
        cmd,
        crate::tui::Cmd::Review { .. } | crate::tui::Cmd::Discard(_)
    )));
    let twice = drive(
        |_| {},
        Vec::new(),
        |app| {
            for _ in 0..2 {
                app.file_revert("src/a.rs");
                app.tree_discard_armed(std::path::PathBuf::from("/work/demo-worktrees/0a1b"));
            }
        },
    );
    let reverted = twice.sent.iter().any(|cmd| {
        matches!(
            cmd,
            crate::tui::Cmd::Review {
                op: crate::hunks::Op::RejectFile,
                ..
            }
        )
    });
    let discarded = twice
        .sent
        .iter()
        .any(|cmd| matches!(cmd, crate::tui::Cmd::Discard(_)));
    assert!(reverted && discarded);
}

#[test]
fn switching_sessions_drops_the_previous_review_marks() {
    let seen = drive(
        |app| {
            app.update(crate::tui::Msg::Fact(Fact::Review {
                root: None,
                result: Ok(crate::hunks::Marks::default()),
            }));
            app.update(crate::tui::Msg::Intent(Intent::Reset));
        },
        Vec::new(),
        |app| app.model.peek().marks.is_none(),
    );
    assert!(seen.read);
}

#[test]
fn the_diff_command_in_the_window_keeps_typing_in_the_composer() {
    if !in_window("window_focus::the_diff_command_in_the_window_keeps_typing_in_the_composer") {
        return;
    }
    let seen = drive(
        |app| app.diff_command(None),
        typed("a"),
        |app| (app.pane_focus.peek(), app.draft.peek()),
    );
    assert_eq!(seen.read, (false, "a".to_owned()));
}

#[test]
fn an_empty_pane_shows_no_zero_count() {
    let seen = drive(|app| app.toggle_pane(Pane::Worktrees), Vec::new(), |_| ());
    assert!(!seen.shows(|text| text == "0"), "{}", seen.text());
}
