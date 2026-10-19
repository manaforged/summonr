use sigil::events::ModifiersState;
use sigil::prelude::Key;

use super::super::Pane;
use super::harness::{command, drive, graded, named, settings, typed, unseated};
use super::in_window;
use crate::mcp::{Field, FieldKind, Form};
use crate::tui::Snapshot;

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
fn command_k_focuses_the_composer_and_escape_drops_its_slash() {
    if !in_window("window_focus::command_k_focuses_the_composer_and_escape_drops_its_slash") {
        return;
    }
    let mut keys = vec![named(Key::Tab, ModifiersState::empty()), command("k")];
    keys.extend(typed("mo"));
    let filtered = drive(settings(graded()), keys, |app| app.draft.peek());
    assert_eq!(filtered.read, "/mo");
    let dropped = drive(
        |_| {},
        vec![command("k"), named(Key::Escape, ModifiersState::empty())],
        |app| app.draft.peek(),
    );
    assert_eq!(dropped.read, "");
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
fn command_one_opens_the_files_pane() {
    if !in_window("window_focus::command_one_opens_the_files_pane") {
        return;
    }
    let seen = drive(
        |_| {},
        vec![command("1")],
        |app| app.layout.peek().secondary,
    );
    assert_eq!(seen.read, Some(Pane::Files));
}

#[test]
fn the_rail_footer_names_the_provider_and_not_the_model() {
    let seen = drive(
        settings(Snapshot {
            endpoint: "stub-provider".to_owned(),
            ..graded()
        }),
        Vec::new(),
        |_| (),
    );
    assert!(
        seen.shows(|text| text == "stub-provider"),
        "{}",
        seen.text()
    );
    let models = seen
        .texts
        .iter()
        .filter(|text| *text == "stub-model")
        .count();
    assert_eq!(models, 1, "{}", seen.text());
}

#[test]
fn the_last_settings_page_shows_its_rows() {
    if !in_window("window_focus::the_last_settings_page_shows_its_rows") {
        return;
    }
    let seen = drive(
        |app| {
            app.toggle_options();
            let rows = crate::tui::sigil_view::settings::setting_rows(app);
            app.settings_sel.set(rows.len() - 1);
        },
        Vec::new(),
        |app| {
            crate::tui::sigil_view::settings::setting_rows(app)
                .last()
                .map(|row| row.label.clone())
                .unwrap_or_default()
        },
    );
    let last = seen.read.clone();
    assert!(
        seen.shows(|text| text.contains(&last)),
        "{last}: {}",
        seen.text()
    );
}

#[test]
fn a_failed_send_with_no_provider_adds_one_notice_and_opens_at_the_top() {
    let seen = drive(settings(unseated()), Vec::new(), |app| {
        for _ in 0..2 {
            app.open_providers();
            app.auth.sel.set(5);
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
        (notices, app.auth.sel.peek())
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
            app.update(crate::tui::Msg::Review {
                root: None,
                result: Ok(crate::hunks::Marks::default()),
            });
            app.update(crate::tui::Msg::Reset);
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
