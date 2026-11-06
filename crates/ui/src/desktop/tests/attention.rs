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

fn one_working(app: &super::super::App) {
    app.update(Msg::Fact(Fact::Live(vec![summonr_agent::hub::Live {
        session: "a".to_owned(),
        title: "a".to_owned(),
        workspace: "/work".into(),
        status: Status::Working,
        active: true,
    }])));
}

#[test]
fn closing_asks_first_while_an_agent_runs() {
    if !in_window("attention::closing_asks_first_while_an_agent_runs") {
        return;
    }
    let seen = drive(one_working, Vec::new(), |_| super::super::quit::on_close());
    assert_eq!(seen.read, sigil::app::CloseAction::Keep);
    assert!(seen.shows(|text| text == "Keep running"), "{}", seen.text());
}

#[test]
fn closing_goes_straight_through_when_nothing_runs() {
    if !in_window("attention::closing_goes_straight_through_when_nothing_runs") {
        return;
    }
    let seen = drive(|_| (), Vec::new(), |_| super::super::quit::on_close());
    assert_eq!(seen.read, sigil::app::CloseAction::Close);
}

#[cfg(feature = "menu")]
#[test]
fn the_menu_bar_runs_the_same_commands_as_the_keys() {
    if !in_window("attention::the_menu_bar_runs_the_same_commands_as_the_keys") {
        return;
    }
    let seen = drive(
        |_| (),
        Vec::new(),
        |_| {
            sigil::app::menu::simulate("New agent");
        },
    );
    assert!(
        seen.sent
            .iter()
            .any(|cmd| matches!(cmd, summonr_agent::hub::Cmd::Open(_))),
        "the menu item must open an agent"
    );
}

#[test]
fn command_shift_u_marks_the_agent_in_view_unread() {
    if !in_window("attention::command_shift_u_marks_the_agent_in_view_unread") {
        return;
    }
    let shift_u = super::harness::press(
        sigil::events::LogicalKey::character("u"),
        sigil::events::ModifiersState::META.union(sigil::events::ModifiersState::SHIFT),
        None,
    );
    let seen = drive(|_| (), vec![shift_u], |_| ());
    assert!(
        seen.sent
            .iter()
            .any(|cmd| matches!(cmd, summonr_agent::hub::Cmd::MarkUnread(_))),
        "the key must mark the agent unread"
    );
}
