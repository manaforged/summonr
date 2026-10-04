use super::*;
use crate::provider::scripted::{Reply, Scripted, Sent};
use crate::provider::{ToolFunction, ToolKind};

struct Run {
    loop_: Loop<Scripted>,
    sent: Sent,
    _dirs: [tempfile::TempDir; 2],
}

fn session(replies: impl IntoIterator<Item = Reply>) -> Run {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let provider = Scripted::new(replies);
    let sent = provider.sent();
    let driver =
        super::super::configure::open_driver(home.path(), workspace.path(), "m", "system").unwrap();
    let loop_ = Loop::open(provider, "m", workspace.path(), "system", driver).unwrap();
    Run {
        loop_,
        sent,
        _dirs: [workspace, home],
    }
}

fn goal(run: &mut Run, review: impl IntoIterator<Item = Reply>) {
    run.loop_.select_goal("ship it", &[]).unwrap();
    run.loop_.set_mode(crate::RunMode::Goal).unwrap();
    run.loop_.set_seat(
        workflow::Role::Review,
        Scripted::new(review),
        "r",
        None,
        None,
    );
}

fn background(command: &str) -> Reply {
    Reply::Call(ToolCall {
        id: "t1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({ "command": command, "background": true }).to_string(),
        },
        continuation: None,
    })
}

fn closing(turn: &Turn) -> Vec<&Event> {
    turn.events
        .iter()
        .filter(|event| {
            matches!(
                event,
                Event::Work { .. }
                    | Event::Note { .. }
                    | Event::Assistant { .. }
                    | Event::Verify { .. }
            )
        })
        .collect()
}

#[tokio::test]
async fn a_plan_turn_reports_where_the_plan_was_saved_then_the_reply() {
    let mut run = session([Reply::Text("the plan")]);
    run.loop_ = run.loop_.with_plan();
    let turn = run.loop_.turn("plan it").await.unwrap();
    let closing = closing(&turn);
    let [
        ..,
        Event::Work { .. },
        Event::Note { text },
        Event::Assistant { text: reply },
    ] = closing.as_slice()
    else {
        panic!("{closing:?}");
    };
    assert!(text.starts_with("plan saved to "), "{text}");
    assert_eq!(reply, "the plan");
}

#[tokio::test]
async fn a_plain_turn_sends_the_prompt_and_ends_with_progress_then_the_reply() {
    let mut run = session([Reply::Text("hello")]);
    let turn = run.loop_.turn("hi there").await.unwrap();
    let sent = run.sent.lock().unwrap().clone();
    let [request] = sent.as_slice() else {
        panic!("{sent:?}");
    };
    assert!(
        request
            .last()
            .is_some_and(|message| message.content.contains("hi there")),
        "{request:?}"
    );
    assert!(turn.checks.is_empty());
    let closing = closing(&turn);
    let [.., Event::Work { .. }, Event::Assistant { text }] = closing.as_slice() else {
        panic!("{closing:?}");
    };
    assert_eq!(text, "hello");
}

#[tokio::test]
async fn a_rejecting_review_fails_the_turn_and_shows_its_findings() {
    let mut run = session([Reply::Text("done")]);
    goal(
        &mut run,
        [Reply::Text(
            r#"{"pass": false, "findings": "the tests were never run"}"#,
        )],
    );
    let turn = run.loop_.turn("go").await.unwrap();
    let review = turn
        .checks
        .iter()
        .find(|check| check.command == crate::review::CHECK);
    assert!(
        review.is_some_and(|check| !check.passed),
        "{:?}",
        turn.checks
    );
    let closing = closing(&turn);
    let at = closing
        .iter()
        .position(|event| matches!(event, Event::Verify { command, passed: false } if command == crate::review::CHECK))
        .unwrap_or_else(|| panic!("{closing:?}"));
    let Event::Note { text } = closing[at + 1] else {
        panic!("{closing:?}");
    };
    assert_eq!(text, "review findings:\nthe tests were never run");
}

#[tokio::test]
async fn an_unreachable_reviewer_fails_the_turn_and_says_why_once() {
    let mut run = session([Reply::Text("done")]);
    goal(&mut run, [Reply::Fail("reviewer offline")]);
    let turn = run.loop_.turn("go").await.unwrap();
    let review = turn
        .checks
        .iter()
        .find(|check| check.command == crate::review::CHECK);
    assert!(
        review.is_some_and(|check| !check.passed),
        "{:?}",
        turn.checks
    );
    let note = closing(&turn).into_iter().find_map(|event| match event {
        Event::Note { text } if text.contains("reviewer offline") => Some(text.clone()),
        _ => None,
    });
    let note = note.unwrap_or_else(|| panic!("{:?}", turn.events));
    assert_eq!(note.matches("review unavailable").count(), 1, "{note}");
}

#[tokio::test]
async fn a_turn_that_leaves_a_job_running_is_deferred_not_verified() {
    let mut run = session([background("sleep 5"), Reply::Text("started it")]);
    goal(&mut run, []);
    let turn = run.loop_.turn("go").await.unwrap();
    assert!(turn.checks.is_empty(), "{:?}", turn.checks);
    let closing = closing(&turn);
    let [
        ..,
        Event::Work { .. },
        Event::Note { text },
        Event::Assistant { text: reply },
    ] = closing.as_slice()
    else {
        panic!("{closing:?}");
    };
    assert!(
        text.starts_with("turn deferred — background work still running: "),
        "{text}"
    );
    assert_eq!(reply, "started it");
}

#[tokio::test]
async fn an_unchanged_harness_note_is_sent_once() {
    let mut run = session([Reply::Text("one"), Reply::Text("two")]);
    run.loop_ = run.loop_.with_plan();
    run.loop_.turn("first").await.unwrap();
    run.loop_.turn("second").await.unwrap();
    let sent = run.sent.lock().unwrap();
    let last_user = |messages: &[Message]| {
        messages
            .iter()
            .rev()
            .find(|message| message.role == crate::provider::Role::User)
            .map(|message| message.content.clone())
            .unwrap_or_default()
    };
    assert_ne!(last_user(&sent[0]), "first");
    assert_eq!(last_user(&sent[1]), "second");
}

#[test]
fn a_partial_reply_at_the_end_is_never_sent_as_the_last_message() {
    let mut run = session([]);
    let tools = run.loop_.advertised_tools();
    let messages = vec![
        Message::system("system".to_string()),
        Message::user("do it".to_string()),
        Message {
            role: Role::Assistant,
            ..Message::system("half a sent".to_string())
        },
    ];
    let request = run
        .loop_
        .request_with(workflow::Role::Implement, &messages, &tools);
    assert_eq!(request.messages.len(), 2);
    assert_eq!(request.messages[1].content, "do it");
}
