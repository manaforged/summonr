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
            agent: None,
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
fn a_chat_run_by_another_agent_shows_only_its_mark() {
    if !in_window("sidebar::a_chat_run_by_another_agent_shows_only_its_mark") {
        return;
    }
    let seen = drive(
        |app| {
            app.window.sidebar.set(true);
            let mut friend = chat(
                "01a1000000000000000000000000cccc",
                "Ask Claude",
                Duration::from_secs(60),
            );
            friend.0.agent = Some("claude".to_owned());
            let own = chat(
                "01a1000000000000000000000000dddd",
                "Ask Summonr",
                Duration::from_secs(60),
            );
            app.update(Msg::Fact(Fact::Sessions(Ok(vec![friend, own]))));
        },
        Vec::new(),
        |_| (),
    );
    let marks = seen.named(sigil::prelude::Role::Image, "Claude Code");
    assert_eq!(marks.len(), 1, "{:?}", seen.controls);
}

#[test]
fn another_agents_chat_keeps_its_icon_and_says_it_is_working() {
    if !in_window("sidebar::another_agents_chat_keeps_its_icon_and_says_it_is_working") {
        return;
    }
    let id = "01a1000000000000000000000000cccc";
    let seen = drive(
        |app| {
            app.window.sidebar.set(true);
            let mut friend = chat(id, "Ask Claude", Duration::from_secs(60));
            friend.0.agent = Some("claude".to_owned());
            app.update(Msg::Fact(Fact::Sessions(Ok(vec![friend]))));
            app.update(Msg::Fact(Fact::Live(vec![summonr_agent::hub::Live {
                session: id.to_owned(),
                title: "Ask Claude".to_owned(),
                workspace: "/work".into(),
                status: summonr_agent::hub::Status::Working,
                active: false,
            }])));
        },
        Vec::new(),
        |_| (),
    );
    assert_eq!(
        seen.named(sigil::prelude::Role::Image, "Claude Code").len(),
        1,
        "{:?}",
        seen.controls
    );
    assert!(seen.shows(|text| text == "working"), "{}", seen.text());
}

#[test]
fn right_clicking_a_chat_opens_it_in_a_split() {
    if !in_window("sidebar::right_clicking_a_chat_opens_it_in_a_split") {
        return;
    }
    let seen = steps(
        two_chats,
        vec![
            Step::RightClick("Fix the parser bug"),
            Step::Click("Open in split"),
        ],
        |_| super::super::split::multiple(),
    );
    assert!(seen.read, "{}", seen.text());
}

#[test]
fn right_clicking_a_repository_offers_its_actions() {
    if !in_window("sidebar::right_clicking_a_repository_offers_its_actions") {
        return;
    }
    let seen = steps(two_chats, vec![Step::RightClick("work")], |_| ());
    for label in ["New agent here", "New agent in split", "Reveal repository"] {
        assert!(seen.shows(|text| text == label), "{label}: {}", seen.text());
    }
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

#[test]
fn a_worktree_agent_lists_under_its_repository() {
    if !in_window("sidebar::a_worktree_agent_lists_under_its_repository") {
        return;
    }
    let seen = drive(
        |app| {
            app.window.sidebar.set(true);
            let mut row = chat(
                "01a1000000000000000000000000dddd",
                "Fix it",
                Duration::from_secs(5),
            );
            row.0.cwd = Some("/work/alpha-worktrees/18e58fff3cdfe408".to_owned());
            app.update(Msg::Fact(Fact::Sessions(Ok(vec![
                row,
                chat(
                    "01a1000000000000000000000000eeee",
                    "Other",
                    Duration::from_secs(9),
                ),
            ]))));
        },
        Vec::new(),
        |_| (),
    );
    assert!(seen.shows(|text| text == "alpha"), "{}", seen.text());
    assert!(
        !seen.shows(|text| text.contains("· worktree")),
        "{}",
        seen.text()
    );
    assert!(seen.shows(|text| text == "Fix it"), "{}", seen.text());
}

fn three_chats(app: &super::super::App) {
    app.window.sidebar.set(true);
    app.update(Msg::Fact(Fact::Sessions(Ok(vec![
        chat(
            "01a1000000000000000000000000aaaa",
            "Keep me",
            Duration::from_secs(5),
        ),
        chat(
            "01a1000000000000000000000000bbbb",
            "Put me away",
            Duration::from_secs(6),
        ),
        chat(
            "01a1000000000000000000000000cccc",
            "Other",
            Duration::from_secs(7),
        ),
    ]))));
}

#[test]
fn a_pinned_agent_moves_to_pinned_and_an_archived_one_leaves_the_list() {
    if !in_window("sidebar::a_pinned_agent_moves_to_pinned_and_an_archived_one_leaves_the_list") {
        return;
    }
    let seen = drive(three_chats, Vec::new(), |_| {
        super::super::shelf::flip_pin("01a1000000000000000000000000aaaa");
        super::super::shelf::archive("01a1000000000000000000000000bbbb");
    });
    let once = |wanted: &str| seen.texts.iter().filter(|text| *text == wanted).count();
    assert_eq!(once("Pinned"), 1, "{}", seen.text());
    assert_eq!(once("Keep me"), 1, "{}", seen.text());
    assert_eq!(once("Put me away"), 0, "{}", seen.text());
    assert_eq!(once("Other"), 1, "{}", seen.text());
}

#[test]
fn unpinning_puts_the_agent_back_and_no_pins_hide_the_heading() {
    if !in_window("sidebar::unpinning_puts_the_agent_back_and_no_pins_hide_the_heading") {
        return;
    }
    let seen = drive(three_chats, Vec::new(), |_| {
        super::super::shelf::flip_pin("01a1000000000000000000000000aaaa");
        super::super::shelf::flip_pin("01a1000000000000000000000000aaaa");
    });
    assert!(!seen.shows(|text| text == "Pinned"), "{}", seen.text());
    assert!(seen.shows(|text| text == "Keep me"), "{}", seen.text());
}

#[test]
fn a_change_in_the_running_agents_refreshes_the_list() {
    if !in_window("sidebar::a_change_in_the_running_agents_refreshes_the_list") {
        return;
    }
    let list_requests = |sent: &[summonr_agent::hub::Cmd]| {
        sent.iter()
            .filter(|cmd| matches!(cmd, summonr_agent::hub::Cmd::ListSessions))
            .count()
    };
    let idle = drive(|app| app.window.sidebar.set(true), Vec::new(), |_| ());
    let seen = drive(
        |app| app.window.sidebar.set(true),
        Vec::new(),
        |app| {
            app.update(Msg::Fact(Fact::Live(vec![summonr_agent::hub::Live {
                session: "01a1000000000000000000000000dddd".to_owned(),
                title: "New work".to_owned(),
                workspace: "/work".into(),
                status: summonr_agent::hub::Status::Working,
                active: true,
            }])));
        },
    );
    assert!(
        list_requests(&seen.sent) > list_requests(&idle.sent),
        "a new running agent must ask for the list again"
    );
}

#[test]
fn search_rows_show_the_repository_and_the_time() {
    if !in_window("sidebar::search_rows_show_the_repository_and_the_time") {
        return;
    }
    let seen = drive(three_chats, Vec::new(), |_| {
        super::super::palette::show();
    });
    assert!(
        seen.shows(|text| text.ends_with(" · now")),
        "{}",
        seen.text()
    );
}
