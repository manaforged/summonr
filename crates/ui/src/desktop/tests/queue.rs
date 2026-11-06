use sigil::prelude::Role;

use super::harness::{Step, drive, steps};
use super::in_window;
use summonr_agent::hub::{Activity, Cmd, Fact, Msg};

fn waiting(app: &super::super::App) {
    app.update(Msg::Fact(Fact::Busy(Some(Activity::Turn))));
    app.update(Msg::Fact(Fact::Queued {
        steering: 1,
        queued: 1,
    }));
    app.update(Msg::Fact(Fact::Pending(vec![
        (true, "use the new parser".to_owned()),
        (false, "then update the docs\nwith examples".to_owned()),
    ])));
}

#[test]
fn messages_typed_during_a_run_wait_above_the_composer() {
    if !in_window("queue::messages_typed_during_a_run_wait_above_the_composer") {
        return;
    }
    let seen = drive(waiting, Vec::new(), |_| ());
    for wanted in [
        "2 messages queued",
        "Next",
        "use the new parser",
        "After this turn",
        "then update the docs",
    ] {
        assert!(
            seen.shows(|text| text == wanted),
            "{wanted}: {}",
            seen.text()
        );
    }
    assert!(
        !seen.shows(|text| text.contains("with examples")),
        "{}",
        seen.text()
    );
    assert_eq!(seen.named(Role::Button, "Send now"), vec![false]);
}

#[test]
fn send_now_steers_the_queue_and_edit_pulls_it_back() {
    if !in_window("queue::send_now_steers_the_queue_and_edit_pulls_it_back") {
        return;
    }
    let sent = steps(waiting, vec![Step::Click("Send now")], |_| ());
    assert!(sent.sent.iter().any(|cmd| matches!(cmd, Cmd::SteerQueued)));
    let edited = steps(waiting, vec![Step::Click("Edit")], |_| ());
    assert!(edited.sent.iter().any(|cmd| matches!(cmd, Cmd::Dequeue)));
}
