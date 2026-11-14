use std::time::{Duration, SystemTime};

use sigil::prelude::Role;
use summonr_agent::forge::{Ask, Bucket, Check, Failure, Pull, Review, State};
use summonr_agent::hub::{Cmd, Fact, Msg};
use summonr_agent::session::SessionInfo;

use super::harness::{Step, changed, steps};
use super::in_window;

const TOPIC: &str = "01a1000000000000000000000000dddd";
const TITLE: &str = "Fix the parser bug";

fn topic(app: &super::super::App) {
    let info = SessionInfo {
        id: TOPIC.to_owned(),
        path: std::path::PathBuf::new(),
        cwd: Some("/work".to_owned()),
        model: Some("m".to_owned()),
        preview: TITLE.to_owned(),
        name: None,
        parent: None,
        depth: 0,
        interactive: true,
        agent: None,
        topic: None,
        modified: SystemTime::now().checked_sub(Duration::from_secs(60)),
        format: summonr_agent::session::FORMAT,
    };
    app.update(Msg::Fact(Fact::Sessions(Ok(vec![(info, true)]))));
    app.update(Msg::Fact(Fact::Session(TOPIC.to_owned())));
    app.update(Msg::Fact(Fact::Event(summonr_agent::Event::User {
        text: TITLE.to_owned(),
        images: Vec::new(),
    })));
    app.update(Msg::Fact(Fact::Diff(
        changed("HEAD", &[("src/parse.rs", 4, 1)]),
        None,
    )));
}

fn pull(checks: Vec<Check>) -> Pull {
    Pull {
        number: 7,
        url: "https://github.com/owner/repo/pull/7".to_owned(),
        state: State::Open,
        review: Review::None,
        checks,
    }
}

fn asks(sent: &[Cmd]) -> Vec<Ask> {
    sent.iter()
        .filter_map(|cmd| match cmd {
            Cmd::Forge(ask) if *ask != Ask::Status => Some(ask.clone()),
            _ => None,
        })
        .collect()
}

fn reply(result: Result<Option<Pull>, Failure>) -> Step {
    Step::Wait(Box::new(move |app| {
        app.update(Msg::Fact(Fact::Forge(result)))
    }))
}

#[test]
fn open_pr_sends_the_topic_title_and_the_reply_shows_the_pr_and_merge() {
    if !in_window("ship::open_pr_sends_the_topic_title_and_the_reply_shows_the_pr_and_merge") {
        return;
    }
    let seen = steps(
        |app| {
            topic(app);
            app.update(Msg::Fact(Fact::Forge(Ok(None))));
        },
        vec![Step::Click("Open PR"), reply(Ok(Some(pull(Vec::new()))))],
        |_| (),
    );
    assert_eq!(asks(&seen.sent), [Ask::Open(TITLE.to_owned())]);
    assert!(seen.shows(|text| text == "#7"), "{}", seen.text());
    assert_eq!(
        seen.named(Role::Button, "Merge"),
        [false],
        "{:?}",
        seen.controls
    );
    assert!(
        seen.named(Role::Button, "Open PR").is_empty(),
        "{:?}",
        seen.controls
    );
}

#[test]
fn signed_out_gh_shows_github_not_signed_in_and_nothing_else() {
    if !in_window("ship::signed_out_gh_shows_github_not_signed_in_and_nothing_else") {
        return;
    }
    let seen = steps(
        |app| {
            topic(app);
            app.update(Msg::Fact(Fact::Forge(Err(Failure::SignedOut))));
        },
        Vec::new(),
        |_| (),
    );
    assert!(
        seen.shows(|text| text == "GitHub not signed in"),
        "{}",
        seen.text()
    );
    assert!(
        seen.named(Role::Button, "Open PR").is_empty(),
        "{:?}",
        seen.controls
    );
    assert!(
        seen.named(Role::Button, "Merge").is_empty(),
        "{:?}",
        seen.controls
    );
    assert!(asks(&seen.sent).is_empty());
}

#[test]
fn a_gh_timeout_shows_in_the_header_and_offers_nothing_else() {
    if !in_window("ship::a_gh_timeout_shows_in_the_header_and_offers_nothing_else") {
        return;
    }
    let timed_out = Failure::TimedOut {
        command: "pr list".to_owned(),
        secs: 30,
    };
    let seen = steps(
        |app| {
            topic(app);
            app.update(Msg::Fact(Fact::Forge(Err(timed_out))));
        },
        Vec::new(),
        |_| (),
    );
    assert!(
        seen.shows(|text| text == "gh pr list did not answer in 30s"),
        "{}",
        seen.text()
    );
    assert!(
        seen.named(Role::Button, "Open PR").is_empty(),
        "{:?}",
        seen.controls
    );
    assert!(
        seen.named(Role::Button, "Merge").is_empty(),
        "{:?}",
        seen.controls
    );
}

#[test]
fn a_failed_check_goes_to_the_agent_with_its_name_and_link() {
    if !in_window("ship::a_failed_check_goes_to_the_agent_with_its_name_and_link") {
        return;
    }
    let failed = Check {
        name: "test (ubuntu)".to_owned(),
        bucket: Bucket::Fail,
        link: "https://github.com/owner/repo/actions/runs/9".to_owned(),
    };
    let passed = Check {
        name: "lint".to_owned(),
        bucket: Bucket::Pass,
        link: "https://github.com/owner/repo/actions/runs/8".to_owned(),
    };
    let seen = steps(
        |app| {
            topic(app);
            app.update(Msg::Fact(Fact::Forge(Ok(Some(pull(vec![failed, passed]))))));
        },
        vec![Step::Click("Checks")],
        |_| (),
    );
    assert!(
        seen.shows(|text| text == "Checks failed"),
        "{}",
        seen.text()
    );
    let prompts: Vec<String> = seen
        .sent
        .iter()
        .filter_map(|cmd| match cmd {
            Cmd::Submit(input) | Cmd::Steer(input) | Cmd::Queue(input) => Some(input.text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(prompts.len(), 1, "{prompts:?}");
    assert!(
        prompts[0].contains("- test (ubuntu): https://github.com/owner/repo/actions/runs/9"),
        "{}",
        prompts[0]
    );
    assert!(!prompts[0].contains("lint"), "{}", prompts[0]);
}

#[test]
fn merge_sends_merge_and_a_merged_reply_archives_the_topic() {
    if !in_window("ship::merge_sends_merge_and_a_merged_reply_archives_the_topic") {
        return;
    }
    let merged = Pull {
        state: State::Merged,
        ..pull(Vec::new())
    };
    let seen = steps(
        |app| {
            topic(app);
            app.update(Msg::Fact(Fact::Forge(Ok(Some(pull(Vec::new()))))));
        },
        vec![Step::Click("Merge"), reply(Ok(Some(merged)))],
        |_| super::super::shelf::archived(),
    );
    assert_eq!(asks(&seen.sent), [Ask::Merge]);
    assert_eq!(seen.read, [TOPIC.to_owned()]);
    assert!(
        seen.named(Role::Button, "Merge").is_empty(),
        "{:?}",
        seen.controls
    );
}
