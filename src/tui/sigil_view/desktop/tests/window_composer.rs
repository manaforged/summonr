use sigil::events::ModifiersState;
use sigil::kernel::Role;
use sigil::prelude::Key;

use super::super::Pane;
use super::harness::{changed, command, drive, graded, named, settings, unseated};
use super::in_window;
use crate::provider::Effort;
use crate::tui::{Activity, Cmd, Msg, Snapshot};

#[test]
fn tab_in_the_window_leaves_effort_and_approval_alone() {
    if !in_window("window_composer::tab_in_the_window_leaves_effort_and_approval_alone") {
        return;
    }
    let seen = drive(
        settings(graded()),
        vec![
            named(Key::Tab, ModifiersState::empty()),
            named(Key::Tab, ModifiersState::SHIFT),
        ],
        |app| app.model.peek().snapshot.effort,
    );
    assert_eq!(seen.read, Some(Effort::Low));
    assert!(
        !seen
            .sent
            .iter()
            .any(|cmd| matches!(cmd, Cmd::SetEffort(_) | Cmd::ToggleApproval))
    );
}

#[test]
fn command_k_opens_the_command_list_and_command_p_a_file_mention() {
    if !in_window("window_composer::command_k_opens_the_command_list_and_command_p_a_file_mention")
    {
        return;
    }
    let listed = drive(|_| {}, vec![command("k")], |app| app.draft.peek());
    assert_eq!(listed.read, "/");
    assert!(listed.shows(|text| text == "commands"), "{}", listed.text());
    let mention = drive(
        |app| app.replace("see".to_owned()),
        vec![command("p")],
        |app| app.draft.peek(),
    );
    assert_eq!(mention.read, "see @");
}

#[test]
fn escape_closes_the_list_and_the_pane_without_cancelling_the_turn() {
    let listed = drive(
        |app| {
            app.update(Msg::Busy(Some(Activity::Turn)));
            app.replace("/se".to_owned());
        },
        vec![named(Key::Escape, ModifiersState::empty())],
        |app| app.suggesting(),
    );
    assert!(!listed.sent.iter().any(|cmd| matches!(cmd, Cmd::Cancel)));
    assert!(!listed.read);
    let paned = drive(
        |app| {
            app.update(Msg::Busy(Some(Activity::Turn)));
            app.toggle_pane(Pane::Files);
        },
        vec![
            named(Key::Escape, ModifiersState::empty()),
            named(Key::Escape, ModifiersState::empty()),
        ],
        |app| app.layout.peek().secondary,
    );
    assert!(!paned.sent.iter().any(|cmd| matches!(cmd, Cmd::Cancel)));
    assert_eq!(paned.read, None);
}

#[test]
fn sending_with_no_provider_opens_the_providers_and_sends_nothing() {
    let seen = drive(settings(unseated()), Vec::new(), |app| {
        app.replace("hello".to_owned());
        app.submit();
        (app.auth.open.peek(), app.draft.peek())
    });
    assert_eq!(seen.read, (true, "hello".to_owned()));
    assert!(
        !seen
            .sent
            .iter()
            .any(|cmd| matches!(cmd, Cmd::Submit(_) | Cmd::Steer(_) | Cmd::Queue(_)))
    );
}

#[test]
fn the_empty_window_with_no_provider_offers_only_add_a_provider() {
    let seen = drive(settings(unseated()), Vec::new(), |_| ());
    assert!(
        seen.shows(|text| text == "Add a provider"),
        "{}",
        seen.text()
    );
    assert!(
        !seen.shows(|text| text == "Explain this repo"),
        "{}",
        seen.text()
    );
}

#[test]
fn an_unknown_command_leaves_an_empty_draft() {
    let seen = drive(
        |_| {},
        Vec::new(),
        |app| {
            app.replace("/nosuchcommand".to_owned());
            app.submit();
            app.draft.peek()
        },
    );
    assert_eq!(seen.read, "");
}

#[test]
fn the_title_bar_drops_the_model_button_and_shows_changes_only_when_dirty() {
    let clean = drive(settings(graded()), Vec::new(), |_| ());
    assert_eq!(
        clean.named(Role::Button, "Model").len(),
        1,
        "{:?}",
        clean.controls
    );
    assert!(
        !clean.shows(|text| text.starts_with("Changes +")),
        "{}",
        clean.text()
    );
    let dirty = drive(
        |app| app.update(Msg::Diff(changed("HEAD", &[("a.rs", 3, 1)]))),
        Vec::new(),
        |_| (),
    );
    assert!(
        dirty.shows(|text| text == "Changes +3 −1"),
        "{}",
        dirty.text()
    );
}

#[test]
fn send_waits_for_text_and_the_approval_chip_names_the_mode() {
    let blank = drive(
        settings(Snapshot {
            approval: true,
            ..graded()
        }),
        Vec::new(),
        |_| (),
    );
    assert_eq!(blank.named(Role::Button, "Send"), vec![true]);
    assert!(
        blank.shows(|text| text == "ask on risky"),
        "{}",
        blank.text()
    );
    let typed = drive(|app| app.replace("go".to_owned()), Vec::new(), |_| ());
    assert_eq!(typed.named(Role::Button, "Send"), vec![false]);
}

#[test]
fn a_mention_in_the_draft_shows_a_removable_chip() {
    let seen = drive(
        |app| app.replace("see @src/a.rs".to_owned()),
        Vec::new(),
        |_| (),
    );
    assert_eq!(
        seen.named(Role::Button, "Remove src/a.rs").len(),
        1,
        "{:?}",
        seen.controls
    );
}

#[test]
fn the_status_row_shows_a_notice() {
    let seen = drive(
        |app| {
            app.notice.set(Some(crate::tui::sigil_view::app::Notice {
                text: "copied 3 lines".to_owned(),
                kind: crate::tui::sigil_view::app::NoticeKind::Copied,
            }));
        },
        Vec::new(),
        |_| (),
    );
    assert!(
        seen.shows(|text| text.contains("copied 3 lines")),
        "{}",
        seen.text()
    );
}

#[test]
fn approval_notes_replace_each_other_instead_of_stacking() {
    let seen = drive(
        |app| {
            app.update(Msg::Approval("approval mode: on".to_owned()));
            app.update(Msg::Approval("auto approval".to_owned()));
        },
        Vec::new(),
        |app| {
            app.model
                .peek()
                .entries
                .iter()
                .filter(|entry| matches!(entry, crate::tui::sigil_view::model::Entry::System(_)))
                .count()
        },
    );
    assert_eq!(seen.read, 1);
}
