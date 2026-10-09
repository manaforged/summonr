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

fn session(
    home: &std::path::Path,
    work: &std::path::Path,
) -> crate::runtime::Loop<crate::provider::Client> {
    let client = crate::provider::Client::new("k", "http://127.0.0.1:9/v1").unwrap();
    let driver =
        crate::durable::driver::SessionDriver::open(home, work, "m", "system", None).unwrap();
    crate::runtime::Loop::open(client, "m", work, "system", driver).unwrap()
}

fn info() -> crate::tui::Info {
    crate::tui::Info {
        version: String::new(),
        model: "m".to_string(),
        relay: None,
        session: String::new(),
        endpoint: "e".to_string(),
        pick: None,
        prefs: crate::prefs::Prefs::default(),
        splash: false,
        inline: false,
        strict: false,
        workspace: String::new(),
        workflow: None,
        config: None,
    }
}

async fn until<T>(
    step: &str,
    inbox: &mut tokio::sync::mpsc::UnboundedReceiver<Msg>,
    pick: impl Fn(&Msg) -> Option<T>,
) -> T {
    let wait = std::time::Duration::from_secs(10);
    tokio::time::timeout(wait, async {
        loop {
            if let Some(found) = pick(&inbox.recv().await.unwrap()) {
                return found;
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("no {step} within the wait"))
}

fn active(msg: &Msg) -> Option<(String, usize)> {
    if let Msg::Live(live) = msg {
        return live
            .iter()
            .find(|entry| entry.active)
            .map(|entry| (entry.session.clone(), live.len()));
    }
    None
}

#[tokio::test(flavor = "multi_thread")]
async fn a_new_session_takes_the_view_and_the_first_comes_back_in_place() {
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let first = session(home.path(), work.path());
    let first_id = first.session_id();
    let (made_home, made_work) = (home.path().to_path_buf(), work.path().to_path_buf());
    let factory: super::Factory =
        std::sync::Arc::new(move |_, _| Ok((session(&made_home, &made_work), None)));
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let started = super::start(first, info(), (tx, rx), factory);
    let mut inbox = started.inbox;
    started
        .commands
        .send(crate::tui::Cmd::Open(super::Open::New))
        .unwrap();
    let wanted = first_id.clone();
    let second = until("the new session as active", &mut inbox, |msg| {
        active(msg)
            .filter(|(id, open)| *id != wanted && *open == 2)
            .map(|(id, _)| id)
    })
    .await;
    until("the new session's id", &mut inbox, |msg| {
        matches!(msg, Msg::Session(id) if *id == second).then_some(())
    })
    .await;
    started
        .commands
        .send(crate::tui::Cmd::Open(super::Open::Resume(first_id.clone())))
        .unwrap();
    until("the reset", &mut inbox, |msg| {
        matches!(msg, Msg::Reset).then_some(())
    })
    .await;
    let wanted = first_id.clone();
    until("the first session as active", &mut inbox, |msg| {
        active(msg).filter(|seen| *seen == (wanted.clone(), 2))
    })
    .await;
    until("the first session's id", &mut inbox, |msg| {
        matches!(msg, Msg::Session(id) if *id == first_id).then_some(())
    })
    .await;
    started.commands.send(crate::tui::Cmd::Quit).unwrap();
    started.task.await.unwrap();
}
