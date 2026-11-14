use sigil::prelude::Role;

use super::harness::{Step, changed, drive, steps};
use super::in_window;
use summonr_agent::hub::{Cmd, Fact, Msg};

fn edited(app: &super::super::App, recent: &[&str]) {
    app.update(Msg::Fact(Fact::Event(summonr_agent::Event::User {
        text: "edit".to_owned(),
        images: Vec::new(),
    })));
    app.update(Msg::Fact(Fact::Diff(
        changed("HEAD", &[("src/a.rs", 10, 2), ("src/b.rs", 3, 1)]),
        None,
    )));
    app.update(Msg::Fact(Fact::Edited(
        recent.iter().map(|path| (*path).to_owned()).collect(),
    )));
}

#[test]
fn a_started_chat_shows_only_a_small_stat_that_opens_changes() {
    if !in_window("changes::a_started_chat_shows_only_a_small_stat_that_opens_changes") {
        return;
    }
    let seen = drive(|app| edited(app, &["src/a.rs"]), Vec::new(), |_| ());
    assert_eq!(
        seen.named(Role::Button, "Changes").len(),
        1,
        "{:?}",
        seen.controls
    );
    assert!(seen.shows(|text| text == "+13 −3"), "{}", seen.text());
    assert!(
        !seen.shows(|text| text.contains("changed")),
        "{}",
        seen.text()
    );
    assert!(
        !seen.shows(|text| text.contains("src/a.rs")),
        "{}",
        seen.text()
    );
    assert!(seen.named(Role::Button, "Undo").is_empty());
}

#[test]
fn a_new_chat_shows_nothing_about_changes() {
    if !in_window("changes::a_new_chat_shows_nothing_about_changes") {
        return;
    }
    let seen = drive(
        |app| {
            app.update(Msg::Fact(Fact::Diff(
                changed("HEAD", &[("src/a.rs", 10, 2)]),
                None,
            )));
        },
        Vec::new(),
        |_| (),
    );
    assert!(seen.named(Role::Button, "Changes").is_empty());
    assert!(!seen.shows(|text| text.contains("+10")), "{}", seen.text());
    assert!(
        !seen.shows(|text| text.contains("changed")),
        "{}",
        seen.text()
    );
}

#[test]
fn undo_waits_for_a_turn_that_edited_files() {
    if !in_window("changes::undo_waits_for_a_turn_that_edited_files") {
        return;
    }
    let seen = drive(
        |app| {
            in_worktree(app);
            app.update(Msg::Fact(Fact::Edited(Vec::new())));
        },
        Vec::new(),
        |_| (),
    );
    assert!(seen.named(Role::Button, "Undo").is_empty());
}

#[test]
fn undo_restores_files_through_the_undo_command() {
    if !in_window("changes::undo_restores_files_through_the_undo_command") {
        return;
    }
    let seen = steps(in_worktree, vec![Step::Click("Undo")], |_| ());
    assert!(
        seen.sent
            .iter()
            .any(|cmd| matches!(cmd, Cmd::Command(line) if line == "/undo files")),
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
            app.update(Msg::Intent(summonr_agent::hub::Intent::Reset));
        },
        Vec::new(),
        |_| (),
    );
    assert!(
        !seen.shows(|text| text.contains("changed")),
        "{}",
        seen.text()
    );
}

fn in_worktree(app: &super::super::App) {
    edited(app, &["src/a.rs"]);
    app.update(Msg::Fact(Fact::Trees(vec![
        summonr_agent::worktree::Status {
            tree: summonr_agent::worktree::Tree {
                primary: "/repo".into(),
                path: app.workspace(),
                branch: "summonr/agent".to_owned(),
                base: "main".to_owned(),
                snapshot: None,
            },
            ahead: summonr_agent::worktree::Ahead(Some(1)),
            dirty: true,
            session: None,
        },
    ])));
    app.toggle_pane(super::super::Pane::Diff);
}

#[test]
fn changes_merges_and_discards_this_agents_worktree() {
    if !in_window("changes::changes_merges_and_discards_this_agents_worktree") {
        return;
    }
    let seen = steps(in_worktree, vec![Step::Click("Merge")], |app| {
        app.workspace()
    });
    assert!(
        seen.shows(|text| text == "summonr/agent → main"),
        "{}",
        seen.text()
    );
    assert!(
        seen.sent
            .iter()
            .any(|cmd| matches!(cmd, Cmd::Merge(path) if *path == seen.read)),
    );
    let armed = steps(
        in_worktree,
        vec![Step::Click("Discard"), Step::Click("Confirm discard")],
        |app| app.workspace(),
    );
    assert!(
        armed
            .sent
            .iter()
            .any(|cmd| matches!(cmd, Cmd::Discard(path) if *path == armed.read)),
    );
}

#[test]
fn a_running_agent_cannot_be_merged_or_discarded() {
    if !in_window("changes::a_running_agent_cannot_be_merged_or_discarded") {
        return;
    }
    let seen = drive(
        |app| {
            in_worktree(app);
            app.update(Msg::Fact(Fact::Busy(Some(
                summonr_agent::hub::Activity::Turn,
            ))));
        },
        Vec::new(),
        |_| (),
    );
    assert!(
        seen.named(Role::Button, "Merge").is_empty(),
        "{:?}",
        seen.controls
    );
    assert!(
        seen.named(Role::Button, "Discard").is_empty(),
        "{:?}",
        seen.controls
    );
}

#[test]
fn a_failed_diff_shows_its_error_in_changes() {
    if !in_window("changes::a_failed_diff_shows_its_error_in_changes") {
        return;
    }
    let seen = drive(
        |app| {
            app.update(Msg::Fact(Fact::DiffFailed(
                "not a git repository".to_owned(),
                None,
            )));
            app.toggle_pane(super::super::Pane::Diff);
        },
        Vec::new(),
        |_| (),
    );
    assert!(
        seen.shows(|text| text.contains("not a git repository")),
        "{}",
        seen.text()
    );
}
