use sigil::prelude::Role;

use super::harness::{Step, changed, drive, steps};
use super::in_window;
use crate::tui::{Cmd, Fact, Msg};

fn edited(app: &super::super::App, recent: &[&str]) {
    app.update(Msg::Fact(Fact::Diff(
        changed("HEAD", &[("src/a.rs", 10, 2), ("src/b.rs", 3, 1)]),
        None,
    )));
    app.update(Msg::Fact(Fact::Edited(recent.iter().map(|path| (*path).to_owned()).collect())));
}

#[test]
fn changed_files_show_a_count_undo_and_a_diff_stat_that_opens_changes() {
    if !in_window("changes::changed_files_show_a_count_undo_and_a_diff_stat_that_opens_changes") {
        return;
    }
    let seen = drive(|app| edited(app, &["src/a.rs"]), Vec::new(), |_| ());
    assert!(seen.shows(|text| text == "2 files changed"), "{}", seen.text());
    assert_eq!(seen.named(Role::Button, "Changes").len(), 1, "{:?}", seen.controls);
    assert!(seen.shows(|text| text == "+13 −3"), "{}", seen.text());
    assert_eq!(seen.named(Role::Button, "Undo").len(), 1, "{:?}", seen.controls);
}

#[test]
fn undo_waits_for_a_turn_that_edited_files() {
    if !in_window("changes::undo_waits_for_a_turn_that_edited_files") {
        return;
    }
    let seen = drive(|app| edited(app, &[]), Vec::new(), |_| ());
    assert!(seen.shows(|text| text == "2 files changed"), "{}", seen.text());
    assert!(seen.named(Role::Button, "Undo").is_empty());
    let clean = drive(|_| (), Vec::new(), |_| ());
    assert!(!clean.shows(|text| text.contains("changed")), "{}", clean.text());
}

#[test]
fn undo_restores_files_through_the_undo_command() {
    if !in_window("changes::undo_restores_files_through_the_undo_command") {
        return;
    }
    let seen = steps(
        |app| edited(app, &["src/a.rs"]),
        vec![Step::Click("Undo")],
        |_| (),
    );
    assert!(
        seen.sent.iter().any(|cmd| matches!(cmd, Cmd::Command(line) if line == "/undo files")),
        "{} commands",
        seen.sent.len()
    );
}

#[test]
fn a_new_chat_drops_the_previous_chats_changes() {
    if !in_window("changes::a_new_chat_drops_the_previous_chats_changes") {
        return;
    }
    let seen = drive(
        |app| {
            edited(app, &["src/a.rs"]);
            app.update(Msg::Intent(crate::tui::Intent::Reset));
        },
        Vec::new(),
        |_| (),
    );
    assert!(!seen.shows(|text| text.contains("changed")), "{}", seen.text());
}
