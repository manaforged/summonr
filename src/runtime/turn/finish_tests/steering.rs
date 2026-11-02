use super::*;

#[path = "steering_mail.rs"]
mod mail;

#[tokio::test]
async fn steering_interrupts_a_pending_provider_request() {
    let steering = Arc::new(Steering::default());
    let sender = Arc::clone(&steering);
    let provider =
        Scripted::new([Reply::Pending, Reply::Text("stopped")]).on_complete(move |round| {
            if round == 0 {
                sender
                    .lock()
                    .unwrap()
                    .push_back(UserInput::from("stop now"));
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
            .any(|event| matches!(event, Event::Assistant { text } if text == "stopped"))
    );
    let sent = run.sent.lock().unwrap();
    assert_eq!(sent.len(), 2);
    assert!(sent[1].iter().any(|message| message.content == "stop now"));
}

#[cfg(unix)]
#[tokio::test]
async fn steering_kills_a_foreground_command_and_skips_the_rest_of_its_batch() {
    let calls = [
        ("active", "touch started; sleep 30; touch escaped"),
        ("stale", "touch stale"),
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
            .push_back(UserInput::from("stop now"));
        steering.notify();
    };
    let turn = run.loop_.turn("start");
    let (report, ()) =
        tokio::time::timeout(Duration::from_secs(5), async { tokio::join!(turn, sender) })
            .await
            .unwrap();
    let report = report.unwrap();
    assert!(!workspace.join("escaped").exists());
    assert!(!workspace.join("stale").exists());
    for id in ["active", "stale"] {
        assert!(report.events.iter().any(|event| matches!(event,
            Event::ToolResult { id: actual, success: false, output, .. }
                if actual == id && output.contains("interrupted by an incoming message")
        )));
    }
    assert!(
        run.sent.lock().unwrap()[1]
            .iter()
            .any(|message| message.content == "stop now")
    );
}
