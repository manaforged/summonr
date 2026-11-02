use super::*;

fn call(name: &str, arguments: serde_json::Value) -> Reply {
    Reply::Call(ToolCall {
        id: name.to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: name.to_string(),
            arguments: arguments.to_string(),
        },
        continuation: None,
    })
}

fn wrote() -> Reply {
    call(
        "write",
        serde_json::json!({"path": "done.txt", "content": "done\n"}),
    )
}

fn judged(replies: impl IntoIterator<Item = Reply>) -> Run {
    let mut run = session(replies);
    let status = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(run._dirs[0].path())
        .status()
        .unwrap();
    assert!(status.success());
    run.loop_.judge_turns = true;
    run
}

fn replies_saying(run: &Run, text: &str) -> usize {
    run.loop_
        .session
        .history()
        .iter()
        .filter(|message| message.role == Role::Assistant && message.content == text)
        .count()
}

const DONE: &str = r#"{"verdict": "done", "reason": "the file exists"}"#;
const MORE: &str = r#"{"verdict": "continue", "reason": "the docs are missing"}"#;

#[tokio::test]
async fn a_judge_that_says_continue_keeps_the_turn_going() {
    let mut run = judged([
        wrote(),
        Reply::Text("first"),
        Reply::Text(MORE),
        Reply::Text("second"),
        Reply::Text(DONE),
    ]);
    let turn = run.loop_.turn("write it").await.unwrap();
    assert_eq!(turn.text, "second");
    assert_eq!(replies_saying(&run, "first"), 1);
    assert_eq!(replies_saying(&run, "second"), 1);
    let sent = run.sent.lock().unwrap();
    assert_eq!(sent.len(), 5, "{sent:?}");
    assert!(sent[2][1].content.contains("first"), "{:?}", sent[2]);
    let note = &sent[3].last().unwrap().content;
    assert!(note.contains("the docs are missing"), "{note}");
}

#[tokio::test]
async fn a_judge_that_says_done_ends_the_turn_and_records_the_reply_once() {
    let mut run = judged([wrote(), Reply::Text("finished"), Reply::Text(DONE)]);
    let turn = run.loop_.turn("write it").await.unwrap();
    assert_eq!(turn.text, "finished");
    assert_eq!(replies_saying(&run, "finished"), 1);
    assert_eq!(run.sent.lock().unwrap().len(), 3);
}

#[tokio::test]
async fn a_judge_failure_reaches_the_user_as_a_note() {
    let mut run = judged([wrote(), Reply::Text("finished"), Reply::Fail("no judge")]);
    let turn = run.loop_.turn("write it").await.unwrap();
    assert_eq!(turn.text, "finished");
    assert!(
        turn.events.iter().any(|event| matches!(
            event,
            Event::Note { text } if text.contains("judge unavailable") && text.contains("no judge")
        )),
        "{:?}",
        turn.events
    );
}

#[tokio::test]
async fn a_user_correction_reaches_the_judge() {
    let steering = Arc::new(Steering::default());
    let late = Arc::clone(&steering);
    let provider = Scripted::new([wrote(), Reply::Text("leaving it"), Reply::Text(DONE)])
        .on_complete(move |call| {
            if call == 0 {
                late.lock().unwrap().push_back(UserInput {
                    text: "stop, leave it".to_string(),
                    images: Vec::new(),
                    source: Some(crate::provider::Source {
                        text: "stop, leave it".to_string(),
                        images: 0,
                    }),
                });
            }
        });
    let mut run = open(provider);
    run.loop_.steer_with(steering);
    run.loop_.judge_turns = true;
    run.loop_.turn("write it").await.unwrap();
    let sent = run.sent.lock().unwrap();
    assert_eq!(sent.len(), 3, "{sent:?}");
    assert!(
        sent[2][1].content.contains("stop, leave it"),
        "{:?}",
        sent[2]
    );
}

#[tokio::test]
async fn tool_calls_between_replies_do_not_reset_the_continuation_cap() {
    let cap = crate::limits::limits().turn_continues;
    let todo = |index: usize| {
        call(
            "todo",
            serde_json::json!({"items": [{"text": format!("step {index}"), "status": "active"}]}),
        )
    };
    let mut script = vec![todo(0)];
    for index in 1..=cap {
        script.push(Reply::Text("working"));
        script.push(todo(index));
    }
    script.push(Reply::Text("still working"));
    let mut run = session(script);
    let turn = run.loop_.turn("do the steps").await.unwrap();
    assert_eq!(turn.text, "still working");
    assert_eq!(run.sent.lock().unwrap().len(), 2 * cap + 2);
    assert!(
        turn.events.iter().any(|event| matches!(
            event,
            Event::Note { text } if text.contains("turn_continues")
        )),
        "{:?}",
        turn.events
    );
}

#[tokio::test]
async fn the_budget_is_checked_before_the_request_after_an_interruption() {
    let steering = Arc::new(Steering::default());
    let sender = Arc::clone(&steering);
    let provider =
        Scripted::new([Reply::Pending, Reply::Text("over budget")]).on_complete(move |round| {
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
    run.loop_.session.set_budget(Some(1), None).unwrap();
    let turn = tokio::time::timeout(Duration::from_secs(5), run.loop_.turn("start"))
        .await
        .unwrap()
        .unwrap();
    assert!(turn.aborted, "{turn:?}");
    assert_eq!(run.sent.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_mail_turn_keeps_the_objective() {
    let mail = tempfile::tempdir().unwrap();
    let open = |id: &str| {
        crate::mail::Mailbox::open(mail.path(), id, Some(id), "summonr", "m", mail.path()).unwrap()
    };
    let (sender, receiver) = (open("sender"), open("receiver"));
    let mut run = session([Reply::Text("fixed"), Reply::Text("read the mail")]);
    run.loop_.mailbox = Some(receiver);
    run.loop_.turn("fix the meter").await.unwrap();
    sender.send("receiver", "the meter is fine now").unwrap();
    run.loop_.mail_poll().unwrap();
    run.loop_.turn_mail_with(|_| {}).await.unwrap();
    assert_eq!(run.loop_.session.objective().unwrap(), "fix the meter");
}
