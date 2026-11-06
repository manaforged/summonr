use std::time::{Duration, SystemTime};

use super::harness::{Step, drive, steps, typed};
use super::in_window;
use summonr_agent::hub::{Fact, Msg};
use summonr_agent::session::SessionInfo;

fn chat(id: &str, preview: &str, age: Duration) -> (SessionInfo, bool) {
    (
        SessionInfo {
            id: id.to_owned(),
            path: std::path::PathBuf::new(),
            cwd: Some("/work".to_owned()),
            model: Some("m".to_owned()),
            preview: preview.to_owned(),
            name: None,
            parent: None,
            depth: 0,
            interactive: true,
            modified: SystemTime::now().checked_sub(age),
        },
        true,
    )
}

fn two_chats(app: &super::super::App) {
    app.window.sidebar.set(true);
    app.update(Msg::Fact(Fact::Sessions(Ok(vec![
        chat(
            "01a1000000000000000000000000aaaa",
            "Write release notes",
            Duration::from_secs(5),
        ),
        chat(
            "01a1000000000000000000000000bbbb",
            "Fix the parser bug",
            Duration::from_secs(3 * 3_600 + 60),
        ),
    ]))));
}

#[test]
fn each_chat_shows_when_it_was_last_active() {
    if !in_window("sidebar::each_chat_shows_when_it_was_last_active") {
        return;
    }
    let seen = drive(two_chats, Vec::new(), |_| ());
    assert!(seen.shows(|text| text == "now"), "{}", seen.text());
    assert!(seen.shows(|text| text == "3h"), "{}", seen.text());
}

#[test]
fn search_finds_an_agent_by_title_in_the_palette() {
    if !in_window("sidebar::search_finds_an_agent_by_title_in_the_palette") {
        return;
    }
    let mut actions = vec![Step::Click("Search")];
    actions.extend(typed("parser").into_iter().map(Step::Key));
    let seen = steps(two_chats, actions, |_| ());
    let menu = seen.named(sigil::prelude::Role::MenuItem, "Fix the parser bug");
    assert_eq!(menu.len(), 1, "{:?}", seen.controls);
    assert!(
        seen.named(sigil::prelude::Role::MenuItem, "Write release notes")
            .is_empty(),
        "{:?}",
        seen.controls
    );
}

#[test]
fn a_live_agent_shows_its_new_title_before_the_list_refreshes() {
    if !in_window("sidebar::a_live_agent_shows_its_new_title_before_the_list_refreshes") {
        return;
    }
    let seen = drive(
        |app| {
            two_chats(app);
            app.update(Msg::Fact(Fact::Live(vec![summonr_agent::hub::Live {
                session: "01a1000000000000000000000000bbbb".to_owned(),
                title: "Parser fix".to_owned(),
                workspace: "/work".into(),
                status: summonr_agent::hub::Status::Idle,
                active: false,
            }])));
        },
        Vec::new(),
        |_| (),
    );
    assert!(seen.shows(|text| text == "Parser fix"), "{}", seen.text());
    assert!(
        !seen.shows(|text| text == "Fix the parser bug"),
        "{}",
        seen.text()
    );
}

#[test]
fn a_repository_lists_before_its_agent_has_a_message() {
    if !in_window("sidebar::a_repository_lists_before_its_agent_has_a_message") {
        return;
    }
    let seen = drive(
        |app| {
            app.window.sidebar.set(true);
            app.update(Msg::Fact(Fact::Sessions(Ok(Vec::new()))));
            app.update(Msg::Fact(Fact::Live(vec![summonr_agent::hub::Live {
                session: "01a1000000000000000000000000cccc".to_owned(),
                title: String::new(),
                workspace: "/added-repo".into(),
                status: summonr_agent::hub::Status::Idle,
                active: false,
            }])));
        },
        Vec::new(),
        |_| (),
    );
    assert!(seen.shows(|text| text == "added-repo"), "{}", seen.text());
}
