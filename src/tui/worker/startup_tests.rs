use super::*;
use crate::durable::driver::SessionDriver;
use crate::provider::Effort;
use tokio::sync::mpsc::unbounded_channel;

fn info(
    loop_: &Loop<Client>,
    workspace: &std::path::Path,
    model: &str,
    config: Option<std::path::PathBuf>,
) -> Info {
    Info {
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
        workspace: workspace.display().to_string(),
        workflow: None,
        config,
    }
}

fn silent_loop(
    home: &std::path::Path,
    workspace: &std::path::Path,
    server: &std::net::TcpListener,
) -> Loop<Client> {
    let base = format!("http://{}/v1", server.local_addr().expect("server address"));
    let client = Client::new("test", &base).expect("local client");
    let driver =
        SessionDriver::open(home, workspace, "gpt-5.4", "system", None).expect("session driver");
    Loop::open(client, "gpt-5.4", workspace, "system", driver).expect("loop")
}

#[test]
fn task_progress_replays_the_current_numbered_plan() {
    let home = tempfile::tempdir().expect("session home");
    let workspace = tempfile::tempdir().expect("workspace");
    let mut driver = SessionDriver::open(home.path(), workspace.path(), "m", "system", None)
        .expect("session driver");
    driver
        .prepare_external_work(
            "## Steps\n1. First task\n2. Second task\n3. Third task\n",
            crate::WorkKind::Steps,
            &[],
        )
        .expect("select plan");
    let (ui, mut inbox) = unbounded_channel();
    history::replay_transcript(driver.path(), Some("fixture".to_string()), None, &ui);
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
    let info = info(&loop_, workspace.path(), model, None);
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
async fn a_model_switch_becomes_the_last_used_model() {
    let home = tempfile::tempdir().expect("summonr home");
    let workspace = tempfile::tempdir().expect("workspace");
    let client = Client::new("test", "http://127.0.0.1:9/v1").expect("local client");
    let driver = SessionDriver::open(home.path(), workspace.path(), "gpt-5.4", "system", None)
        .expect("session driver");
    let loop_ = Loop::open(client, "gpt-5.4", workspace.path(), "system", driver).expect("loop");
    let info = info(
        &loop_,
        workspace.path(),
        "gpt-5.4",
        Some(home.path().join("config.toml")),
    );
    let (commands, receiver) = unbounded_channel();
    let (ui, _inbox) = unbounded_channel();
    commands
        .send(Cmd::SetModel("gpt-5.5".to_string()))
        .expect("set model");
    commands.send(Cmd::Quit).expect("quit");
    worker(loop_, receiver, ui, info).await;
    assert_eq!(
        crate::recent::load(home.path()).first(),
        Some(&crate::recent::ModelId::new("openai", "gpt-5.5"))
    );
}

#[tokio::test]
async fn a_session_opened_with_an_unfinished_goal_says_how_to_go_on() {
    let home = tempfile::tempdir().expect("session home");
    let workspace = tempfile::tempdir().expect("workspace");
    let client = Client::new("test", "http://127.0.0.1:9/v1").expect("local client");
    let driver = SessionDriver::open(home.path(), workspace.path(), "m", "system", None)
        .expect("session driver");
    let mut loop_ = Loop::open(client, "m", workspace.path(), "system", driver).expect("loop");
    loop_
        .select_goal("add a --version flag", &[])
        .expect("goal");
    loop_.set_mode(crate::WorkKind::Single);
    let info = info(&loop_, workspace.path(), "m", None);
    let (commands, receiver) = unbounded_channel();
    let (ui, mut inbox) = unbounded_channel();
    commands.send(Cmd::Quit).expect("quit");
    worker(loop_, receiver, ui, info).await;
    let notes: Vec<String> = std::iter::from_fn(|| inbox.try_recv().ok())
        .filter_map(|msg| match msg {
            Msg::System(text) => Some(text),
            _msg => None,
        })
        .collect();
    assert!(
        notes
            .iter()
            .any(|text| text.contains("add a --version flag")
                && text.contains("same text")
                && text.contains("/goal clear")),
        "{notes:?}"
    );
}

#[tokio::test]
async fn a_goal_with_verify_off_takes_no_gates_from_the_check_file() {
    let home = tempfile::tempdir().expect("session home");
    let workspace = tempfile::tempdir().expect("workspace");
    let checks = workspace.path().join(".summonr");
    std::fs::create_dir_all(&checks).expect("check folder");
    std::fs::write(checks.join("check.toml"), "check = \"false\"\n").expect("check file");
    let server = std::net::TcpListener::bind("127.0.0.1:0").expect("silent server");
    let loop_ = silent_loop(home.path(), workspace.path(), &server);
    let info = info(&loop_, workspace.path(), "gpt-5.4", None);
    let (commands, receiver) = unbounded_channel();
    let (ui, mut inbox) = unbounded_channel();
    commands
        .send(Cmd::Command("/goal ship it".to_string()))
        .expect("goal");
    commands.send(Cmd::Quit).expect("quit");
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        worker(loop_, receiver, ui, info),
    )
    .await
    .expect("quit ends the goal");
    let notes: Vec<String> = std::iter::from_fn(|| inbox.try_recv().ok())
        .filter_map(|msg| match msg {
            Msg::System(text) => Some(text),
            _msg => None,
        })
        .collect();
    assert!(
        notes.iter().any(|text| text.starts_with("check: none")),
        "Verify is off, so the goal runs without the check file's gates: {notes:?}"
    );
}

#[tokio::test]
async fn an_aborted_goal_turn_runs_the_exit_check_before_it_stops() {
    let home = tempfile::tempdir().expect("session home");
    let workspace = tempfile::tempdir().expect("workspace");
    let server = std::net::TcpListener::bind("127.0.0.1:0").expect("silent server");
    let budget = crate::Budget {
        max_tokens: None,
        max_secs: Some(1),
    };
    let loop_ = silent_loop(home.path(), workspace.path(), &server)
        .with_verify(vec!["false".to_string()])
        .and_then(|loop_| loop_.with_budget(budget))
        .expect("checked loop");
    let info = info(&loop_, workspace.path(), "gpt-5.4", None);
    let (commands, receiver) = unbounded_channel();
    let (ui, mut inbox) = unbounded_channel();
    commands
        .send(Cmd::Command("/goal ship it".to_string()))
        .expect("goal");
    let watch = async {
        let mut verified = false;
        while let Some(msg) = inbox.recv().await {
            match msg {
                Msg::Event(crate::Event::Verify { passed, .. }) => verified |= !passed,
                Msg::System(text) if text.contains("stopped") => break,
                Msg::Outcome(_) => break,
                _msg => {}
            }
        }
        drop(commands.send(Cmd::Quit));
        verified
    };
    let ((), verified) = tokio::time::timeout(std::time::Duration::from_secs(30), async {
        tokio::join!(worker(loop_, receiver, ui, info), watch)
    })
    .await
    .expect("the time budget ends the goal");
    assert!(verified, "the stopped goal reports its failing check");
}
