use std::sync::Arc;
use std::time::Duration;

use super::*;

#[tokio::test]
async fn input_typed_while_a_command_runs_reports_the_waiting_steer() {
    let dir = tempfile::tempdir().unwrap();
    let (ui, mut inbox) = tokio::sync::mpsc::unbounded_channel();
    let reader = Reader::new(
        dir.path().to_path_buf(),
        Default::default(),
        ui.clone(),
        true,
    );
    let steering: Arc<crate::runtime::Steering> = Arc::default();
    steering
        .lock()
        .unwrap()
        .push_back(UserInput::from("check the logs"));
    let mut queue = InputQueue::new(steering);
    let (send, mut cmd) = tokio::sync::mpsc::unbounded_channel();
    send.send(Cmd::Queue(UserInput::from("then run the tests")))
        .unwrap();
    let outcome = wait_cancelable(
        &mut cmd,
        &mut queue,
        &reader,
        &ui,
        Waiting::Work,
        None,
        tokio::time::sleep(Duration::from_millis(200)),
    )
    .await;
    assert!(matches!(outcome, Completion::Done(())));
    let counts: Vec<(usize, usize)> = std::iter::from_fn(|| inbox.try_recv().ok())
        .filter_map(|message| match message {
            Msg::Fact(Fact::Queued { steering, queued }) => Some((steering, queued)),
            _other => None,
        })
        .collect();
    assert_eq!(counts, [(1, 1)]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_panel_command_runs_off_the_worker_and_reports_its_chart() {
    let dir = tempfile::tempdir().unwrap();
    let panels = dir.path().join(".summonr").join("panels");
    std::fs::create_dir_all(&panels).unwrap();
    std::fs::write(
        panels.join("build.toml"),
        r#"command = ["sh", "-c", "printf '{\"view\":\"text\",\"text\":\"hello\"}'"]"#,
    )
    .unwrap();
    let (ui, mut inbox) = tokio::sync::mpsc::unbounded_channel();

    let reader = Reader::new(
        dir.path().to_path_buf(),
        Default::default(),
        ui.clone(),
        true,
    );
    reader.run(Cmd::ListPanels).await;
    let Some(Msg::Fact(crate::hub::Fact::Panels(list))) = inbox.recv().await else {
        panic!("no panel list");
    };
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "build");
    reader.run(Cmd::RunPanel("build".to_string())).await;
    let Some(Msg::Fact(crate::hub::Fact::Panel { name, result })) = inbox.recv().await else {
        panic!("no panel result");
    };
    assert_eq!(name, "build");
    assert_eq!(
        result,
        Ok(crate::panel::Content::Text {
            text: "hello".to_string()
        })
    );

    let untrusted = Reader::new(dir.path().to_path_buf(), Default::default(), ui, false);
    untrusted.run(Cmd::ListPanels).await;
    let Some(Msg::Fact(crate::hub::Fact::Panels(list))) = inbox.recv().await else {
        panic!("no panel list");
    };
    assert!(list.is_empty());
    untrusted.run(Cmd::RunPanel("build".to_string())).await;
    let Some(Msg::Fact(crate::hub::Fact::Panel { result, .. })) = inbox.recv().await else {
        panic!("no panel result");
    };
    assert_eq!(result, Err("no panel named `build`".to_string()));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_side_panel_lists_rows_whose_action_starts_an_agent() {
    let dir = tempfile::tempdir().expect("workspace");
    let panels = dir.path().join(".summonr").join("panels");
    std::fs::create_dir_all(&panels).expect("panels folder");
    std::fs::write(
        panels.join("team.toml"),
        r##"place = "side"
command = ["sh", "-c", "printf '%s' '{\"view\":\"list\",\"rows\":[{\"title\":\"Checkout total off by a cent\",\"meta\":\"#212\",\"tone\":\"warning\",\"action\":{\"start\":{\"prompt\":\"Fix issue #212\"}}}]}'"]"##,
    )
    .expect("panel file");
    let (ui, mut inbox) = tokio::sync::mpsc::unbounded_channel();
    let reader = Reader::new(dir.path().to_path_buf(), Default::default(), ui, true);
    reader.run(Cmd::ListPanels).await;
    let Some(Msg::Fact(crate::hub::Fact::Panels(list))) = inbox.recv().await else {
        panic!("no panel list");
    };
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].place, crate::panel::Place::Side);
    reader.run(Cmd::RunPanel("team".to_string())).await;
    let Some(Msg::Fact(crate::hub::Fact::Panel { result, .. })) = inbox.recv().await else {
        panic!("no panel result");
    };
    assert_eq!(
        result,
        Ok(crate::panel::Content::List {
            rows: vec![crate::panel::Item {
                title: "Checkout total off by a cent".to_string(),
                meta: Some("#212".to_string()),
                tone: crate::panel::Mood::Warning,
                action: Some(crate::panel::Action::Start {
                    prompt: "Fix issue #212".to_string()
                }),
            }]
        })
    );
}

fn panel_reader(files: &[(&str, &str)]) -> (tempfile::TempDir, Reader, tokio::sync::mpsc::UnboundedReceiver<Msg>) {
    let dir = tempfile::tempdir().expect("workspace");
    let panels = dir.path().join(".summonr").join("panels");
    std::fs::create_dir_all(&panels).expect("panels folder");
    for (name, body) in files {
        std::fs::write(panels.join(format!("{name}.toml")), body).expect("panel file");
    }
    let (ui, inbox) = tokio::sync::mpsc::unbounded_channel();
    let reader = Reader::new(dir.path().to_path_buf(), Default::default(), ui, true);
    (dir, reader, inbox)
}

async fn panel_fact(inbox: &mut tokio::sync::mpsc::UnboundedReceiver<Msg>) -> (String, Result<crate::panel::Content, String>) {
    loop {
        let message = tokio::time::timeout(Duration::from_secs(10), inbox.recv())
            .await
            .expect("a panel result within 10s")
            .expect("hub open");
        if let Msg::Fact(crate::hub::Fact::Panel { name, result }) = message {
            return (name, result);
        }
    }
}

const SURFACE: &str = r#"[{"version":"v0.9.1","updateComponents":{"surfaceId":"queue","components":[{"id":"root","component":"Text","text":"Nothing to review"}]}}]"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_panel_that_prints_a2ui_reports_the_surface_as_printed() {
    let file = format!("command = [\"printf\", \"%s\", '{SURFACE}']");
    let (_dir, reader, mut inbox) = panel_reader(&[("queue", &file)]);
    reader.run(Cmd::RunPanel("queue".to_string())).await;
    let (name, result) = panel_fact(&mut inbox).await;
    assert_eq!(name, "queue");
    assert_eq!(result, Ok(crate::panel::Content::Surface(SURFACE.to_string())));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_panel_event_runs_the_command_again_with_the_action_on_stdin() {
    let file = format!("command = [\"sh\", \"-c\", \"cat > action.json; printf '%s' '{}'\"]", SURFACE.replace('"', "\\\""));
    let (dir, reader, mut inbox) = panel_reader(&[("queue", &file)]);
    reader
        .run(Cmd::ActPanel {
            name: "queue".to_string(),
            event: "approve".to_string(),
            context: serde_json::json!({"pr": 12}),
        })
        .await;
    let (name, result) = panel_fact(&mut inbox).await;
    assert_eq!(name, "queue");
    assert_eq!(result, Ok(crate::panel::Content::Surface(SURFACE.to_string())));
    let sent: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.path().join("action.json")).expect("the command read stdin"),
    )
    .expect("the action is JSON");
    let action = &sent["action"];
    assert_eq!(action["name"], "approve");
    assert_eq!(action["surfaceId"], "queue");
    assert_eq!(action["context"], serde_json::json!({"pr": 12}));
    assert!(
        action["timestamp"].as_str().is_some_and(|stamp| stamp.parse::<jiff::Timestamp>().is_ok()),
        "an ISO 8601 timestamp: {action}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn three_panels_asked_at_once_each_report() {
    let slow = r#"command = ["sh", "-c", "sleep 0.3; printf '{\"view\":\"text\",\"text\":\"ok\"}'"]"#;
    let (_dir, reader, mut inbox) = panel_reader(&[("a", slow), ("b", slow), ("c", slow)]);
    for name in ["a", "b", "c"] {
        reader.run(Cmd::RunPanel(name.to_string())).await;
    }
    let mut names = Vec::new();
    for _ in 0..3 {
        names.push(panel_fact(&mut inbox).await.0);
    }
    names.sort();
    assert_eq!(names, ["a", "b", "c"]);
}
