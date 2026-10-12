use super::*;
use crate::durable::driver::SessionDriver;
use crate::provider::Effort;
use tokio::sync::mpsc::unbounded_channel;

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
