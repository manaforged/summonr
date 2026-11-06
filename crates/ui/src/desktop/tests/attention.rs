use super::super::attention::alert;
use super::harness::{drive, settings};
use super::in_window;
use summonr_agent::hub::{Fact, Msg, Status};
use summonr_agent::prefs::Notify;

fn asked(detail: &str) -> Msg {
    Msg::Fact(Fact::Event(summonr_agent::Event::Approval {
        detail: Some(detail.to_owned()),
        persist: false,
        dialog: 0,
    }))
}

#[test]
fn an_approval_and_a_finished_turn_call_the_user_back() {
    if !in_window("attention::an_approval_and_a_finished_turn_call_the_user_back") {
        return;
    }
    let seen = drive(
        settings(summonr_agent::hub::Snapshot::default()),
        Vec::new(),
        |app| {
            let idle_end = app.attention(&Msg::Fact(Fact::Busy(None)));
            app.update(Msg::Fact(Fact::Busy(Some(
                summonr_agent::hub::Activity::Turn,
            ))));
            (
                app.attention(&asked("git push --force origin main\nrewrites history")),
                app.attention(&Msg::Fact(Fact::Busy(None))),
                idle_end,
            )
        },
    );
    let (approval, finished, idle_end) = seen.read;
    assert_eq!(
        approval,
        Some((
            "Needs you".to_owned(),
            "git push --force origin main".to_owned()
        ))
    );
    assert_eq!(
        finished.map(|(title, _)| title).as_deref(),
        Some("Summonr finished")
    );
    assert_eq!(idle_end, None, "a turn that never started does not notify");
}

#[test]
fn notify_me_decides_what_calls_the_user_back() {
    if !in_window("attention::notify_me_decides_what_calls_the_user_back") {
        return;
    }
    let cases = [
        (Notify::Off, false, false),
        (Notify::Turn, false, true),
        (Notify::All, true, true),
    ];
    for (notify, asks, ends) in cases {
        let seen = drive(
            settings(summonr_agent::hub::Snapshot::default()),
            Vec::new(),
            move |app| {
                app.flip(|prefs| prefs.notify = notify);
                app.update(Msg::Fact(Fact::Busy(Some(
                    summonr_agent::hub::Activity::Turn,
                ))));
                (
                    app.attention(&asked("rm -rf build")).is_some(),
                    app.attention(&Msg::Fact(Fact::Busy(None))).is_some(),
                )
            },
        );
        assert_eq!(seen.read, (asks, ends), "{notify:?} in the open pane");
        assert_eq!(
            alert(Status::NeedsYou, notify).is_some(),
            asks,
            "{notify:?} out of view"
        );
        assert_eq!(
            alert(Status::Review, notify).is_some(),
            ends,
            "{notify:?} out of view"
        );
    }
}

#[test]
fn the_sidebar_toggle_counts_the_agents_that_need_you() {
    if !in_window("attention::the_sidebar_toggle_counts_the_agents_that_need_you") {
        return;
    }
    let live = |id: &str, status| summonr_agent::hub::Live {
        session: id.to_owned(),
        title: id.to_owned(),
        workspace: "/work".into(),
        status,
        active: false,
    };
    let seen = drive(
        move |app| {
            app.update(Msg::Fact(Fact::Live(vec![
                live("a", Status::NeedsYou),
                live("b", Status::Working),
                live("c", Status::NeedsYou),
            ])));
        },
        Vec::new(),
        |_| (),
    );
    assert!(seen.shows(|text| text == "2"), "{}", seen.text());
}
