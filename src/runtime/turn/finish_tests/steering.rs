use super::*;

#[path = "steering_mail.rs"]
mod mail;

#[tokio::test]
async fn steering_waits_for_the_reply_in_a_main_session() {
    let steering = Arc::new(Steering::default());
    let sender = Arc::clone(&steering);
    let provider =
        Scripted::new([Reply::Text("first"), Reply::Text("second")]).on_complete(move |round| {
            if round == 0 {
                sender
                    .lock()
                    .unwrap()
                    .push_back(UserInput::from("also this"));
                sender.notify();
            }
        });
    let mut run = open(provider);
    run.loop_.steer_with(steering);
    let report = tokio::time::timeout(Duration::from_secs(5), run.loop_.turn("start"))
        .await
        .unwrap()
        .unwrap();
    assert!(
        report
            .events
            .iter()
            .any(|event| matches!(event, Event::Assistant { text } if text == "first"))
    );
    let sent = run.sent.lock().unwrap();
    assert_eq!(sent.len(), 2);
    assert!(sent[1].iter().any(|message| message.content == "also this"));
}

#[cfg(unix)]
#[tokio::test]
async fn steering_lets_a_command_and_its_batch_finish_in_a_main_session() {
    let calls = [
        ("active", "touch started; sleep 0.2; touch finished"),
        ("next", "touch next"),
    ]
    .into_iter()
    .map(|(id, command)| ToolCall {
        id: id.to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({"command": command}).to_string(),
        },
        continuation: None,
    })
    .collect();
    let mut run = session([Reply::Calls(calls), Reply::Text("stopped")]);
    let steering = Arc::new(Steering::default());
    run.loop_.steer_with(Arc::clone(&steering));
    let workspace = run._dirs[0].path().to_path_buf();
    let sender = async {
        while !workspace.join("started").exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        steering
            .lock()
            .unwrap()
            .push_back(UserInput::from("also this"));
        steering.notify();
    };
    let turn = run.loop_.turn("start");
    let (report, ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(turn, sender)
    })
    .await
    .unwrap();
    report.unwrap();
    assert!(workspace.join("finished").exists());
    assert!(workspace.join("next").exists());
    assert!(
        run.sent.lock().unwrap()[1]
            .iter()
            .any(|message| message.content == "also this")
    );
}
