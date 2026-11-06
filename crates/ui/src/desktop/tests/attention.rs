use super::harness::{drive, settings};
use super::in_window;
use summonr_agent::hub::{Fact, Msg};

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
            "Summonr needs your OK".to_owned(),
            "git push --force origin main".to_owned()
        ))
    );
    assert_eq!(
        finished.map(|(title, _)| title).as_deref(),
        Some("Summonr finished")
    );
    assert_eq!(idle_end, None, "a turn that never started does not notify");
}
