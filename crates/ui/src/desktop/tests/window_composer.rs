use sigil::events::ModifiersState;
use sigil::prelude::Key;
use sigil::prelude::Role;

use super::super::Pane;
use super::harness::{command, drive, graded, named, settings, unconfigured};
use super::in_window;
use summonr_agent::hub::{Activity, Cmd, Fact, Msg, Snapshot};
use summonr_agent::provider::Effort;

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
fn command_k_opens_the_palette_and_command_p_a_file_mention() {
    if !in_window("window_composer::command_k_opens_the_palette_and_command_p_a_file_mention") {
        return;
    }
    let listed = drive(|_| {}, vec![command("k")], |app| app.draft.peek());
    assert_eq!(listed.read, "");
    assert_eq!(
        listed
            .named(sigil::prelude::Role::MenuItem, "New agent")
            .len(),
        1,
        "{:?}",
        listed.controls
    );
    let mention = drive(
        |app| app.replace("see".to_owned()),
        vec![command("p")],
        |app| app.draft.peek(),
    );
    assert_eq!(mention.read, "see @");
}

#[test]
fn escape_closes_each_layer_and_never_stops_the_agent() {
    if !in_window("window_composer::escape_closes_each_layer_and_never_stops_the_agent") {
        return;
    }
    let listed = drive(
        |app| {
            app.update(Msg::Fact(Fact::Busy(Some(Activity::Turn))));
            app.replace("/se".to_owned());
        },
        vec![named(Key::Escape, ModifiersState::empty())],
        |app| app.suggesting(),
    );
    assert!(!listed.sent.iter().any(|cmd| matches!(cmd, Cmd::Cancel)));
    assert!(!listed.read);
    let paned = drive(
        |app| {
            app.update(Msg::Fact(Fact::Busy(Some(Activity::Turn))));
            app.toggle_pane(Pane::Files);
        },
        vec![named(Key::Escape, ModifiersState::empty())],
        |app| app.layout.peek().secondary,
    );
    assert!(!paned.sent.iter().any(|cmd| matches!(cmd, Cmd::Cancel)));
    assert_eq!(paned.read, None);
}

#[test]
fn sending_with_no_provider_opens_the_providers_and_sends_nothing() {
    let seen = drive(settings(unconfigured()), Vec::new(), |app| {
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
fn the_empty_window_with_no_provider_offers_only_connect_a_model() {
    let seen = drive(settings(unconfigured()), Vec::new(), |_| ());
    assert!(
        seen.shows(|text| text == "Connect a model"),
        "{}",
        seen.text()
    );
    assert!(
        !seen.shows(|text| text == "Find and fix a bug"),
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
fn send_waits_for_text() {
    let blank = drive(
        settings(Snapshot {
            approval: true,
            ..graded()
        }),
        Vec::new(),
        |_| (),
    );
    assert_eq!(blank.named(Role::Button, "Send"), vec![true]);
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
            app.notice.set(Some(crate::app::Notice {
                text: "copied 3 lines".to_owned(),
                kind: crate::app::NoticeKind::Copied,
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
            app.update(Msg::Fact(Fact::Approval("approval mode: on".to_owned())));
            app.update(Msg::Fact(Fact::Approval("auto approval".to_owned())));
        },
        Vec::new(),
        |app| {
            app.model
                .peek()
                .entries
                .iter()
                .filter(|entry| matches!(entry, crate::model::Entry::System(_)))
                .count()
        },
    );
    assert_eq!(seen.read, 1);
}
