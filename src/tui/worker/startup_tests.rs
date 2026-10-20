use super::*;
use crate::durable::driver::SessionDriver;
use crate::provider::Effort;
use tokio::sync::mpsc::unbounded_channel;

#[test]
fn task_progress_replays_the_current_numbered_plan() {
    let home = tempfile::tempdir().expect("session home");
    let workspace = tempfile::tempdir().expect("workspace");
    let mut driver = SessionDriver::open(home.path(), workspace.path(), "m", "system", None)
        .expect("session driver");
    driver
        .prepare_external_work(
            "## Steps\n1. First task\n2. Second task\n3. Third task\n",
            crate::durable::work::Policy::Steps,
            &[],
        )
        .expect("select plan");
    let (ui, mut inbox) = unbounded_channel();
    history::reshow_running(driver.path(), "fixture", &ui);
    let progress = std::iter::from_fn(|| inbox.try_recv().ok())
        .find_map(|message| match message {
            Msg::Event(crate::Event::Work { progress }) => Some(progress),
            _ => None,
        })
        .expect("replay must restore the checkpoint task progress");
    assert_eq!(progress.current, Some(1));
    assert_eq!(progress.total, 3);
    assert_eq!(progress.ledger[0], "[/] First task");
}

#[tokio::test]
async fn startup_effort_reaches_the_composer_before_a_model_switch() {
    let home = tempfile::tempdir().expect("session home");
    let workspace = tempfile::tempdir().expect("workspace");
    let model = "gpt-5.4";
    let client = Client::new("test", "http://127.0.0.1:9/v1").expect("local client");
    let driver = SessionDriver::open(home.path(), workspace.path(), model, "system", None)
        .expect("session driver");
    let loop_ = Loop::open(client, model, workspace.path(), "system", driver).expect("loop");
    let info = Info {
        version: String::new(),
        model: model.to_string(),
        relay: None,
        session: loop_.session_id(),
        endpoint: "openai".to_string(),
        pick: None,
        prefs: crate::prefs::Prefs::default(),
        splash: false,
        inline: false,
        strict: false,
        workspace: workspace.path().display().to_string(),
        workflow: None,
        config: None,
    };
    let (commands, receiver) = unbounded_channel();
    let (ui, mut inbox) = unbounded_channel();
    commands
        .send(Cmd::SetEffort(Some(Effort::High)))
        .expect("set effort");
    commands.send(Cmd::Quit).expect("quit");
    worker(loop_, receiver, ui, info).await;
    let settings: Vec<_> = std::iter::from_fn(|| inbox.try_recv().ok())
        .filter_map(|msg| match msg {
            Msg::Settings(snapshot) => Some(snapshot),
            _msg => None,
        })
        .collect();
    assert_eq!(settings.len(), 2);
    assert_eq!(settings[0].model, "gpt-5.4");
    assert_eq!(
        settings[0].levels,
        [Effort::Low, Effort::Medium, Effort::High, Effort::XHigh]
    );
    assert_eq!(settings[1].effort, Some(Effort::High));
    assert_eq!(settings[1].levels, settings[0].levels);
}

#[tokio::test]
async fn a_model_switch_becomes_the_last_used_seat() {
    let home = tempfile::tempdir().expect("summonr home");
    let workspace = tempfile::tempdir().expect("workspace");
    let client = Client::new("test", "http://127.0.0.1:9/v1").expect("local client");
    let driver = SessionDriver::open(home.path(), workspace.path(), "gpt-5.4", "system", None)
        .expect("session driver");
    let loop_ = Loop::open(client, "gpt-5.4", workspace.path(), "system", driver).expect("loop");
    let info = Info {
        version: String::new(),
        model: "gpt-5.4".to_string(),
        relay: None,
        session: loop_.session_id(),
        endpoint: "openai".to_string(),
        pick: None,
        prefs: crate::prefs::Prefs::default(),
        splash: false,
        inline: false,
        strict: false,
        workspace: workspace.path().display().to_string(),
        workflow: None,
        config: Some(home.path().join("config.toml")),
    };
    let (commands, receiver) = unbounded_channel();
    let (ui, _inbox) = unbounded_channel();
    commands
        .send(Cmd::SetModel("gpt-5.5".to_string()))
        .expect("set model");
    commands.send(Cmd::Quit).expect("quit");
    worker(loop_, receiver, ui, info).await;
    assert_eq!(
        crate::recent::load(home.path()).first(),
        Some(&crate::recent::Seat::new("openai", "gpt-5.5"))
    );
}
