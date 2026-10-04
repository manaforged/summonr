use super::{Status, seen, status};
use crate::tools::Event;
use crate::tui::{Activity, Msg};

fn approval(detail: Option<&str>) -> Msg {
    Msg::Event(Event::Approval {
        detail: detail.map(str::to_string),
        persist: false,
    })
}

#[test]
fn session_status_follows_the_turn_and_its_questions() {
    let cases = [
        (
            Status::Idle,
            Msg::Busy(Some(Activity::Turn)),
            Status::Working,
        ),
        (
            Status::Working,
            approval(Some("rm -rf build")),
            Status::NeedsYou,
        ),
        (Status::NeedsYou, approval(None), Status::Working),
        (Status::NeedsYou, Msg::Busy(None), Status::Review),
        (Status::Working, Msg::Busy(None), Status::Review),
        (Status::Idle, Msg::Busy(None), Status::Idle),
        (
            Status::Working,
            Msg::Failed("provider down".into()),
            Status::Failed,
        ),
        (
            Status::Idle,
            Msg::Failed("no such session".into()),
            Status::Idle,
        ),
        (Status::Failed, Msg::Busy(None), Status::Failed),
        (Status::Idle, approval(None), Status::Idle),
        (Status::Review, Msg::System("note".into()), Status::Review),
        (
            Status::Working,
            Msg::Event(Event::Elicit { form: None }),
            Status::Working,
        ),
    ];
    for (prev, msg, want) in cases {
        assert_eq!(status(prev, &msg), want, "from {prev:?}");
    }
}

#[test]
fn viewing_a_session_clears_finished_states_only() {
    assert_eq!(seen(Status::Review), Status::Idle);
    assert_eq!(seen(Status::Failed), Status::Idle);
    assert_eq!(seen(Status::NeedsYou), Status::NeedsYou);
    assert_eq!(seen(Status::Working), Status::Working);
}

#[test]
fn switching_back_restores_the_turn_then_its_open_question() {
    let mut sticky = super::sticky::Sticky::default();
    sticky.record(&Msg::Busy(Some(Activity::Turn)), false);
    sticky.record(&approval(Some("git push --force")), false);
    let (app, mut inbox) = tokio::sync::mpsc::unbounded_channel();
    sticky.resend(&app);
    let first = inbox.try_recv().unwrap();
    assert!(matches!(first, Msg::Busy(Some(Activity::Turn))));
    let mut last = None;
    while let Ok(msg) = inbox.try_recv() {
        last = Some(msg);
    }
    assert!(matches!(
        last,
        Some(Msg::Event(Event::Approval { detail: Some(ref detail), .. })) if detail == "git push --force"
    ));
}

#[test]
fn an_answered_question_is_not_asked_again_on_switch() {
    let mut sticky = super::sticky::Sticky::default();
    sticky.record(&Msg::Busy(Some(Activity::Turn)), false);
    sticky.record(&approval(Some("rm -rf build")), false);
    sticky.record(&approval(None), false);
    let (app, mut inbox) = tokio::sync::mpsc::unbounded_channel();
    sticky.resend(&app);
    while let Ok(msg) = inbox.try_recv() {
        assert!(!matches!(msg, Msg::Event(Event::Approval { .. })));
    }
}

#[test]
fn text_handed_back_while_away_returns_once_on_switch() {
    let restored = || Msg::Restored(vec![crate::provider::UserInput::from("finish the docs")]);
    let handed = |sticky: &mut super::sticky::Sticky| {
        let (app, mut inbox) = tokio::sync::mpsc::unbounded_channel();
        sticky.resend(&app);
        std::iter::from_fn(|| inbox.try_recv().ok())
            .filter(|msg| matches!(msg, Msg::Restored(_)))
            .count()
    };
    let mut away = super::sticky::Sticky::default();
    away.record(&restored(), false);
    assert_eq!(handed(&mut away), 1);
    assert_eq!(handed(&mut away), 0);
    let mut shown = super::sticky::Sticky::default();
    shown.record(&restored(), true);
    assert_eq!(handed(&mut shown), 0);
}
