use super::*;
use crate::provider::scripted::{Reply, Scripted, Sent};
use crate::provider::{ToolFunction, ToolKind};

struct Run {
    loop_: Loop<Scripted>,
    sent: Sent,
    _dirs: [tempfile::TempDir; 2],
}

fn session(replies: impl IntoIterator<Item = Reply>) -> Run {
    open(Scripted::new(replies))
}

fn open(provider: Scripted) -> Run {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
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
    run.loop_.set_mode(crate::RunMode::Goal);
    run.loop_.set_seat(
        workflow::Role::Review,
        Scripted::new(review),
        "r",
        None,
        None,
    );
}

fn stalled(check: &str) -> Run {
    let mut run = session([
        Reply::Text("tried"),
        Reply::Text(r#"{"verdict": "continue", "reason": "not yet"}"#),
    ]);
    run.loop_
        .select_goal("make the tests pass", &[check.to_string()])
        .unwrap();
    run
}

fn steered(first: Reply) -> Run {
    let steering = Arc::new(Steering::default());
    let late = Arc::clone(&steering);
    let provider = Scripted::new([first, Reply::Text("steered answer")]).on_complete(move |call| {
        if call == 0 {
            late.lock()
                .unwrap()
                .push_back(UserInput::from("also check the docs"));
        }
    });
    let mut run = open(provider);
    run.loop_.steer_with(steering);
    run
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
        text.starts_with("turn deferred; background work still running: "),
        "{text}"
    );
    assert_eq!(reply, "started it");
}

#[tokio::test]
async fn an_unchanged_harness_note_is_sent_once() {
    let mut run = session([Reply::Text("one"), Reply::Text("two")]);
    run.loop_.set_mode(crate::RunMode::Work);
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

#[tokio::test]
async fn a_goal_selected_from_the_command_line_runs_its_check_and_asks_the_judge() {
    let mut run = session([
        Reply::Text("fixed"),
        Reply::Text(r#"{"verdict": "done", "reason": "the check passed"}"#),
    ]);
    run.loop_
        .select_goal("make the tests pass", &["exit 0".to_string()])
        .unwrap();
    let turn = run.loop_.turn("make the tests pass").await.unwrap();
    assert!(
        turn.events.iter().any(
            |event| matches!(event, Event::Verify { command, passed: true } if command == "exit 0")
        ),
        "{:?}",
        closing(&turn)
    );
    assert_eq!(run.loop_.work_progress().unwrap().exit_code(), Some(0));
}

#[tokio::test]
async fn the_judge_refuses_a_gamed_pass_and_contradictory_tests_stop_the_goal() {
    let mut run = session([
        Reply::Text("I made add stateful so both tests pass"),
        Reply::Text(
            r#"{"verdict": "impossible", "reason": "test_five and test_six expect 5 and 6 for add(2, 3)"}"#,
        ),
    ]);
    run.loop_
        .select_goal("make the tests pass", &["exit 0".to_string()])
        .unwrap();
    run.loop_.turn("make the tests pass").await.unwrap();
    let sent = run.sent.lock().unwrap().clone();
    let system = &sent[1][0].content;
    assert!(system.contains("hidden state, call order"), "{system}");
    assert!(system.contains("a stub or shim on PATH"), "{system}");
    assert!(
        system.contains("reply `impossible` and name both tests"),
        "{system}"
    );
    assert_ne!(run.loop_.work_progress().unwrap().exit_code(), Some(0));
}

#[tokio::test]
async fn steps_selected_from_the_command_line_run_the_completion_check() {
    let mut run = session([Reply::Text("done")]);
    run.loop_
        .select_steps("## Steps\n\n1. Ship it\n", &["exit 0".to_string()])
        .unwrap();
    let turn = run.loop_.turn("Ship it").await.unwrap();
    assert!(
        turn.events.iter().any(
            |event| matches!(event, Event::Verify { command, passed: true } if command == "exit 0")
        ),
        "{:?}",
        closing(&turn)
    );
}

#[tokio::test]
async fn a_goal_that_stops_short_exits_on_its_check() {
    for (check, code) in [("exit 1", 2), ("exit 0", 3)] {
        let mut run = stalled(check);
        run.loop_.turn("make the tests pass").await.unwrap();
        let exit = run
            .loop_
            .stop_work("goal execution aborted", |_| {})
            .await
            .unwrap();
        assert_eq!(exit, code, "check `{check}`");
    }
}

#[tokio::test]
async fn a_failed_check_reads_the_same_in_progress_and_verify() {
    let mut run = stalled("false");
    let turn = run.loop_.turn("make the tests pass").await.unwrap();
    assert!(
        turn.events
            .iter()
            .any(|event| matches!(event, Event::Verify { passed: false, .. })),
        "{:?}",
        closing(&turn)
    );
    assert!(run.loop_.work_progress().unwrap().checks_failed);
}

#[tokio::test]
async fn a_check_file_changed_during_a_run_is_put_back() {
    let mut run = stalled("false");
    let path = run.loop_.workspace().join(".summonr/check.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "check = \"false\"\n").unwrap();
    run.loop_.set_verify(Some(vec!["false".to_string()]));
    std::fs::write(&path, "check = \"true\"\n").unwrap();
    run.loop_.turn("make the tests pass").await.unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "check = \"false\"\n"
    );
}

#[tokio::test]
async fn a_steer_sent_during_the_last_round_reaches_the_model_in_that_turn() {
    let mut run = steered(Reply::Text("first answer"));
    let turn = run.loop_.turn("start").await.unwrap();
    assert_eq!(turn.text, "steered answer");
    assert!(format!("{:?}", turn.events).contains("first answer"));
    let sent = run.sent.lock().unwrap();
    assert_eq!(sent.len(), 2);
    assert!(format!("{:?}", sent[1]).contains("also check the docs"));
}

#[tokio::test]
async fn todo_harness_note_never_shows_in_the_users_own_message() {
    let mut run = session([Reply::Text("done")]);
    run.loop_.set_seat(
        workflow::Role::Review,
        Scripted::new([Reply::Text(r#"{"pass": true}"#)]),
        "r",
        None,
        None,
    );
    run.loop_.turn("ship the parser").await.unwrap();
    let sent = format!("{:?}", run.sent.lock().unwrap()[0]);
    assert!(sent.contains("Harness: a reviewer"), "{sent}");
    let exported = crate::session::export(run.loop_.session.path()).unwrap();
    assert!(exported.contains("ship the parser"), "{exported}");
    assert!(!exported.contains("Harness"), "{exported}");
}

#[tokio::test]
async fn a_steer_sent_while_a_tool_runs_reaches_the_model_after_the_tool_result() {
    let mut run = steered(background("echo hi"));
    let turn = run.loop_.turn("start").await.unwrap();
    assert_eq!(turn.text, "steered answer");
    let sent = run.sent.lock().unwrap();
    assert_eq!(sent.len(), 2);
    let second = format!("{:?}", sent[1]);
    let result = second.find("tool_call_id: Some(\"t1\")");
    let steer = second.find("also check the docs");
    assert!(
        matches!((result, steer), (Some(result), Some(steer)) if result < steer),
        "{second}"
    );
}
