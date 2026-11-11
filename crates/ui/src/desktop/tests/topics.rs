use std::time::{Duration, SystemTime};

use super::harness::{Step, steps};
use super::in_window;
use sigil::prelude::Role;
use summonr_agent::hub::{Cmd, Fact, Msg, Open};
use summonr_agent::session::SessionInfo;

const LEAD: &str = "01a1000000000000000000000000aaaa";
const WORKER: &str = "01a1000000000000000000000000bbbb";
const OTHER: &str = "01a1000000000000000000000000cccc";

fn thread(
    id: &str,
    preview: &str,
    agent: Option<&str>,
    topic: Option<&str>,
) -> (SessionInfo, bool) {
    let info = SessionInfo {
        id: id.to_owned(),
        path: std::path::PathBuf::new(),
        cwd: Some("/work".to_owned()),
        model: Some("m".to_owned()),
        preview: preview.to_owned(),
        name: None,
        parent: None,
        depth: 0,
        interactive: true,
        agent: agent.map(str::to_owned),
        topic: topic.map(str::to_owned),
        modified: SystemTime::now().checked_sub(Duration::from_secs(60)),
    };
    (info, true)
}

fn topic_with_two_agents(app: &super::super::App) {
    app.window.sidebar.set(true);
    app.update(Msg::Fact(Fact::Sessions(Ok(vec![
        thread(LEAD, "Add CSV export", None, None),
        thread(WORKER, "Export button", Some("claude"), Some(LEAD)),
        thread(OTHER, "Fix the parser bug", None, None),
    ]))));
    app.update(Msg::Fact(Fact::Session(LEAD.to_owned())));
}

#[test]
fn threads_in_one_topic_share_a_sidebar_row_and_show_as_tabs() {
    if !in_window("topics::threads_in_one_topic_share_a_sidebar_row_and_show_as_tabs") {
        return;
    }
    let seen = steps(topic_with_two_agents, Vec::new(), |_| ());
    let rows: Vec<&str> = seen
        .controls
        .iter()
        .filter(|(role, _, _)| *role == Role::ListItem)
        .map(|(_, name, _)| name.as_str())
        .collect();
    assert!(rows.contains(&"Add CSV export"), "{rows:?}");
    assert!(rows.contains(&"Fix the parser bug"), "{rows:?}");
    assert!(!rows.contains(&"Export button"), "{rows:?}");
    assert_eq!(
        seen.named(Role::Image, "Claude Code").len(),
        1,
        "{:?}",
        seen.controls
    );
    assert_eq!(
        seen.named(Role::Image, "Summonr").len(),
        2,
        "{:?}",
        seen.controls
    );
    assert_eq!(
        seen.named(Role::Tab, "Add CSV export").len(),
        1,
        "{:?}",
        seen.controls
    );
    assert_eq!(
        seen.named(Role::Tab, "Export button").len(),
        1,
        "{:?}",
        seen.controls
    );
    assert!(seen.named(Role::Tab, "Fix the parser bug").is_empty());
}

#[test]
fn a_tab_opens_its_thread_and_new_thread_joins_the_topic() {
    if !in_window("topics::a_tab_opens_its_thread_and_new_thread_joins_the_topic") {
        return;
    }
    let seen = steps(
        topic_with_two_agents,
        vec![Step::Click("Export button"), Step::Click("New thread")],
        |_| (),
    );
    let opened: Vec<&Open> = seen
        .sent
        .iter()
        .filter_map(|cmd| match cmd {
            Cmd::Open(open) => Some(open),
            _ => None,
        })
        .collect();
    assert!(
        opened.contains(&&Open::Resume(WORKER.to_owned())),
        "{opened:?}"
    );
    assert!(
        opened.contains(&&Open::Thread {
            topic: LEAD.to_owned(),
            folder: "/work".into()
        }),
        "{opened:?}"
    );
}
