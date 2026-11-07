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
