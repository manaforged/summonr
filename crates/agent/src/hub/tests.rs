use super::{Status, seen, status};
use crate::hub::{Activity, Fact, Intent, Msg};
use crate::tools::Event;

fn approval(detail: Option<&str>) -> Msg {
    Msg::Fact(Fact::Event(Event::Approval {
        detail: detail.map(str::to_string),
        id: None,
        persist: false,
        dialog: 1,
    }))
}

#[test]
fn session_status_follows_the_turn_and_its_questions() {
    let cases = [
        (
            Status::Idle,
            Msg::Fact(Fact::Busy(Some(Activity::Turn))),
            Status::Working,
        ),
        (
            Status::Working,
            approval(Some("rm -rf build")),
            Status::NeedsYou,
        ),
        (Status::NeedsYou, approval(None), Status::Working),
        (
            Status::NeedsYou,
            Msg::Fact(Fact::Busy(None)),
            Status::Review,
        ),
        (Status::Working, Msg::Fact(Fact::Busy(None)), Status::Review),
        (Status::Idle, Msg::Fact(Fact::Busy(None)), Status::Idle),
        (
            Status::Working,
            Msg::Fact(Fact::Failed("provider down".into())),
            Status::Failed,
        ),
        (
            Status::Idle,
            Msg::Fact(Fact::Failed("no such session".into())),
            Status::Idle,
        ),
        (Status::Failed, Msg::Fact(Fact::Busy(None)), Status::Failed),
        (Status::Idle, approval(None), Status::Idle),
        (
            Status::Review,
            Msg::Fact(Fact::Event(Event::Note {
                text: "note".into(),
            })),
            Status::Review,
        ),
        (
            Status::Working,
            Msg::Fact(Fact::Event(Event::Elicit {
                form: None,
                dialog: 1,
            })),
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
    let mut replay = super::replay::Replay::default();
    replay.record(&Msg::Fact(Fact::Busy(Some(Activity::Turn))), false);
    replay.record(&approval(Some("git push --force")), false);
    let (app, mut inbox) = tokio::sync::mpsc::unbounded_channel();
    replay.resend(&app);
    let first = inbox.try_recv().unwrap();
    assert!(matches!(first, Msg::Fact(Fact::Busy(Some(Activity::Turn)))));
    let mut last = None;
    while let Ok(msg) = inbox.try_recv() {
        last = Some(msg);
    }
    assert!(matches!(
        last,
        Some(Msg::Fact(Fact::Event(Event::Approval { detail: Some(ref detail), .. }))) if detail == "git push --force"
    ));
}

#[test]
fn an_answered_question_is_not_asked_again_on_switch() {
    let mut replay = super::replay::Replay::default();
    replay.record(&Msg::Fact(Fact::Busy(Some(Activity::Turn))), false);
    replay.record(&approval(Some("rm -rf build")), false);
    replay.record(&approval(None), false);
    let (app, mut inbox) = tokio::sync::mpsc::unbounded_channel();
    replay.resend(&app);
    while let Ok(msg) = inbox.try_recv() {
        assert!(!matches!(
            msg,
            Msg::Fact(Fact::Event(Event::Approval { .. }))
        ));
    }
}

#[test]
fn text_handed_back_while_away_returns_once_on_switch() {
    let restored = || {
        Msg::Fact(Fact::Restored(vec![crate::provider::UserInput::from(
            "finish the docs",
        )]))
    };
    let handed = |replay: &mut super::replay::Replay| {
        let (app, mut inbox) = tokio::sync::mpsc::unbounded_channel();
        replay.resend(&app);
        std::iter::from_fn(|| inbox.try_recv().ok())
            .filter(|msg| matches!(msg, Msg::Fact(Fact::Restored(_))))
            .count()
    };
    let mut away = super::replay::Replay::default();
    away.record(&restored(), false);
    assert_eq!(handed(&mut away), 1);
    assert_eq!(handed(&mut away), 0);
    let mut shown = super::replay::Replay::default();
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

fn info() -> crate::hub::Info {
    crate::hub::Info {
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
        prompt: None,
        trust: true,
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
    if let Msg::Fact(Fact::Live(live)) = msg {
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
        .send(crate::hub::Cmd::Open(super::Open::New))
        .unwrap();
    let wanted = first_id.clone();
    let second = until("the new session as active", &mut inbox, |msg| {
        active(msg)
            .filter(|(id, open)| *id != wanted && *open == 2)
            .map(|(id, _)| id)
    })
    .await;
    until("the new session's id", &mut inbox, |msg| {
        matches!(msg, Msg::Fact(Fact::Session(id)) if *id == second).then_some(())
    })
    .await;
    started
        .commands
        .send(crate::hub::Cmd::Open(super::Open::Resume(first_id.clone())))
        .unwrap();
    let wanted = first_id.clone();
    until("the first session as active", &mut inbox, |msg| {
        assert!(!matches!(msg, Msg::Intent(Intent::Reset)));
        active(msg).filter(|seen| *seen == (wanted.clone(), 2))
    })
    .await;
    until("the first session's transcript", &mut inbox, |msg| {
        assert!(!matches!(msg, Msg::Intent(Intent::Reset)));
        matches!(msg, Msg::Fact(Fact::Transcript { replace: true, .. })).then_some(())
    })
    .await;
    until("the first session's id", &mut inbox, |msg| {
        matches!(msg, Msg::Fact(Fact::Session(id)) if *id == first_id).then_some(())
    })
    .await;
    started.commands.send(crate::hub::Cmd::Quit).unwrap();
    started.task.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn naming_a_session_publishes_its_title() {
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let first = session(home.path(), work.path());
    let (made_home, made_work) = (home.path().to_path_buf(), work.path().to_path_buf());
    let factory: super::Factory =
        std::sync::Arc::new(move |_, _| Ok((session(&made_home, &made_work), None)));
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let started = super::start(first, info(), (tx, rx), factory);
    let mut inbox = started.inbox;
    started
        .commands
        .send(crate::hub::Cmd::Command("/name fix tests".to_string()))
        .unwrap();
    until("the named title", &mut inbox, |msg| {
        let Msg::Fact(Fact::Live(live)) = msg else {
            return None;
        };
        live.iter()
            .any(|entry| entry.active && entry.title == "fix tests")
            .then_some(())
    })
    .await;
    started.commands.send(crate::hub::Cmd::Quit).unwrap();
    started.task.await.unwrap();
}

#[test]
fn the_first_line_of_a_submitted_prompt_titles_the_session() {
    let submit = |text: &str| crate::hub::Cmd::Submit(crate::provider::UserInput::from(text));
    assert_eq!(
        super::run::title_from(&submit("\n  fix the tests\nthen add --json")).as_deref(),
        Some("fix the tests")
    );
    assert_eq!(super::run::title_from(&submit("   ")), None);
    assert_eq!(super::run::title_from(&crate::hub::Cmd::Tick), None);
}

fn live_ids(msg: &Msg) -> Option<Vec<String>> {
    let Msg::Fact(Fact::Live(live)) = msg else {
        return None;
    };
    Some(live.iter().map(|entry| entry.session.clone()).collect())
}

#[tokio::test(flavor = "multi_thread")]
async fn delete_refuses_an_agent_in_view_and_closes_one_out_of_view() {
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
    let send = |command| started.commands.send(command).unwrap();
    send(crate::hub::Cmd::DeleteSession(first_id.clone()));
    until("the refusal", &mut inbox, |msg| {
        matches!(msg, Msg::Fact(Fact::Failed(text)) if text.contains("close its pane"))
            .then_some(())
    })
    .await;
    send(crate::hub::Cmd::Open(super::Open::New));
    let wanted = first_id.clone();
    until("the second agent", &mut inbox, |msg| {
        active(msg)
            .filter(|(id, open)| *id != wanted && *open == 2)
            .map(|_| ())
    })
    .await;
    send(crate::hub::Cmd::DeleteSession(first_id.clone()));
    let wanted = first_id.clone();
    until("the first agent closed", &mut inbox, |msg| {
        live_ids(msg)
            .filter(|ids| ids.len() == 1 && !ids.contains(&wanted))
            .map(|_| ())
    })
    .await;
    send(crate::hub::Cmd::Quit);
    started.task.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn renaming_an_agent_out_of_view_keeps_the_view() {
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
    let send = |command| started.commands.send(command).unwrap();
    send(crate::hub::Cmd::Open(super::Open::New));
    let wanted = first_id.clone();
    until("the second agent", &mut inbox, |msg| {
        active(msg)
            .filter(|(id, open)| *id != wanted && *open == 2)
            .map(|_| ())
    })
    .await;
    send(crate::hub::Cmd::Rename {
        session: first_id.clone(),
        name: "older work".to_owned(),
    });
    let wanted = first_id.clone();
    until("the new title out of view", &mut inbox, |msg| {
        let Msg::Fact(Fact::Live(live)) = msg else {
            return None;
        };
        live.iter()
            .any(|entry| entry.session == wanted && entry.title == "older work" && !entry.active)
            .then_some(())
    })
    .await;
    send(crate::hub::Cmd::Quit);
    started.task.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_child_session_names_its_lead_in_the_roster() {
    let home = tempfile::tempdir().unwrap();
    let work = tempfile::tempdir().unwrap();
    let lead = session(home.path(), work.path()).session_id();
    let client = crate::provider::Client::new("k", "http://127.0.0.1:9/v1").unwrap();
    let parent = crate::durable::driver::Parent::Task(lead.parse().unwrap());
    let driver = crate::durable::driver::SessionDriver::open(
        home.path(),
        work.path(),
        "m",
        "system",
        Some(parent),
    )
    .unwrap();
    let child = crate::runtime::Loop::open(client, "m", work.path(), "system", driver).unwrap();
    let factory: super::Factory = std::sync::Arc::new(|_, _| {
        Err(summonr_core::error::Error::Config(
            "no second agent".to_owned(),
        ))
    });
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let started = super::start(child, info(), (tx, rx), factory);
    let mut inbox = started.inbox;
    let named = until("the roster", &mut inbox, |msg| {
        let Msg::Fact(Fact::Live(live)) = msg else {
            return None;
        };
        live.first().map(|entry| entry.parent.clone())
    })
    .await;
    assert_eq!(named, Some(lead));
    started.commands.send(crate::hub::Cmd::Quit).unwrap();
    started.task.await.unwrap();
}

mod isolate;
