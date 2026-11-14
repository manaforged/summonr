use super::*;
use crate::hooks::{Dialect, Hook, HookEvent};
use crate::provider::{Client, ToolFunction, ToolKind};

fn session(workspace: &Path, home: &Path) -> Loop<Client> {
    let client = Client::new("k", "http://127.0.0.1:9/v1").unwrap();
    let driver = super::super::configure::open_driver(home, workspace, "m", "system").unwrap();
    Loop::open(client, "m", workspace, "system", driver).unwrap()
}

#[cfg(unix)]
const DELETE: &str = "rm -rf";
#[cfg(windows)]
const DELETE: &str = "Remove-Item -Recurse -Force";

fn command(text: &str) -> ToolCall {
    ToolCall {
        id: "t1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "run_command".to_string(),
            arguments: serde_json::json!({
                "command": text,
                "timeout_secs": crate::limits::limits().command_kill().as_secs(),
            })
            .to_string(),
        },
        continuation: None,
    }
}

pub(super) fn hooked(loop_: &mut Loop<Client>, event: HookEvent, dialect: Dialect, command: &str) {
    loop_.hooks.push(
        event,
        Hook {
            command: command.to_string(),
            dialect,
            matcher: None,
            root: None,
            timeout: None,
        },
    );
}

async fn delete_outside(
    steering: Option<Arc<Steering>>,
    answers: &[&str],
) -> (bool, String, Vec<Event>) {
    delete_hooked(&[], steering, answers).await
}

async fn delete_hooked(
    hooks: &[(HookEvent, Dialect, &str)],
    steering: Option<Arc<Steering>>,
    answers: &[&str],
) -> (bool, String, Vec<Event>) {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let target = workspace.path().to_path_buf();
    let mut loop_ = session(workspace.path(), home.path()).with_unrestricted_commands();
    for (event, dialect, command) in hooks {
        hooked(&mut loop_, *event, *dialect, command);
    }
    let mut reply: Box<dyn FnMut(&Event)> = match steering {
        Some(steering) => {
            loop_.steer_with(Arc::clone(&steering));
            Box::new(steering.replying(answers))
        }
        None => Box::new(|_: &Event| {}),
    };
    let call = command(&format!("{DELETE} {}", target.display()));
    let output = loop_.tools.execute_outcome(&call).await;
    let mut events = Vec::new();
    let output = loop_
        .with_grant(
            output,
            &call,
            &mut events,
            &mut reply,
            &mut Usage::default(),
        )
        .await
        .unwrap();
    let text = output.map_or_else(|error| error.to_string(), |output| output.text);
    (target.exists(), text, events)
}

fn user() -> Arc<Steering> {
    Arc::new(Steering::default())
}

#[cfg(unix)]
#[tokio::test]
async fn queued_steering_leaves_the_approval_open_in_a_main_session() {
    let steering = user();
    steering
        .lock()
        .unwrap()
        .push_back(UserInput::from("also update the docs"));
    let (exists, text, _) = delete_outside(Some(Arc::clone(&steering)), &["n"]).await;
    assert!(exists, "{text}");
    let waiting: Vec<String> = steering
        .lock()
        .unwrap()
        .iter()
        .map(|input| input.text.clone())
        .collect();
    assert_eq!(waiting, ["also update the docs"]);
}

#[cfg(unix)]
#[tokio::test]
async fn steering_answer_left_from_an_earlier_dialog_never_answers_the_next() {
    let steering = user();
    steering.answer(steering.open_dialog(), UserInput::from("y"));
    let (exists, text, _) = delete_outside(Some(Arc::clone(&steering)), &["n"]).await;
    assert!(exists, "{text}");
}

#[tokio::test]
async fn a_destructive_command_asks_the_user_first() {
    let (exists, text, events) = delete_outside(Some(user()), &["n"]).await;
    assert!(exists, "{text}");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Approval { detail: Some(detail), .. } if detail.contains(DELETE))),
        "{events:?}"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_confirmed_destructive_command_runs() {
    let (exists, text, _) = delete_outside(Some(user()), &["y"]).await;
    assert!(!exists, "{text}");
}

#[cfg(unix)]
#[tokio::test]
async fn steering_lets_an_approved_command_finish_in_a_main_session() {
    let workspace = tempfile::tempdir().expect("workspace");
    let home = tempfile::tempdir().expect("session home");
    let target = tempfile::tempdir().expect("deletion target");
    let mut loop_ = session(workspace.path(), home.path()).with_unrestricted_commands();
    let steering = user();
    loop_.steer_with(Arc::clone(&steering));
    let call = command(&format!(
        "{DELETE} '{}'; touch started; sleep 0.2; touch finished",
        target.path().display()
    ));
    let output = loop_.tools.execute_outcome(&call).await;
    assert!(crate::tools::needed_grant(&output).is_some());
    let sender = async {
        while !workspace.path().join("started").exists() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        steering
            .lock()
            .expect("steering queue")
            .push_back(UserInput::from("stop now"));
        steering.notify();
    };
    let mut events = Vec::new();
    let mut reply = steering.replying(&["y"]);
    let mut usage = Usage::default();
    let executing = loop_.with_grant(output, &call, &mut events, &mut reply, &mut usage);
    let (result, ()) = tokio::time::timeout(Duration::from_secs(10), async {
        tokio::join!(executing, sender)
    })
    .await
    .expect("the approved command must finish");
    result.expect("grant result").expect("command output");
    assert!(!target.path().exists());
    assert!(workspace.path().join("finished").exists());
}

#[tokio::test]
async fn a_destructive_command_never_runs_without_a_user() {
    let (exists, text, _) = delete_outside(None, &[]).await;
    assert!(exists, "{text}");
}

#[tokio::test]
async fn a_repeated_failure_reaches_the_model_unchanged() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut loop_ = session(workspace.path(), home.path());
    let call = command("make test");
    for _ in 0..8 {
        let text = loop_
            .tool_text(
                &call,
                false,
                "exit 2 make: *** No rule to make target".to_string(),
                None,
            )
            .await;
        assert_eq!(text, "exit 2 make: *** No rule to make target");
    }
}

#[cfg(unix)]
const ALLOW: &str = r#"printf '%s' '{"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "allow"}}}'"#;

fn asked(events: &[Event]) -> bool {
    events.iter().any(|event| {
        matches!(
            event,
            Event::Approval {
                detail: Some(_),
                ..
            }
        )
    })
}

#[cfg(unix)]
#[tokio::test]
async fn a_permission_request_hook_that_allows_answers_without_a_prompt() {
    let hooks = [(HookEvent::PermissionRequest, Dialect::Claude, ALLOW)];
    let (exists, text, events) = delete_hooked(&hooks, Some(user()), &[]).await;
    assert!(!exists, "{text}");
    assert!(!asked(&events), "{events:?}");
}

#[cfg(unix)]
#[tokio::test]
async fn a_permission_request_hook_runs_where_no_one_can_answer() {
    let hooks = [(HookEvent::PermissionRequest, Dialect::Claude, ALLOW)];
    let (exists, text, _) = delete_hooked(&hooks, None, &[]).await;
    assert!(!exists, "{text}");
}

#[cfg(unix)]
#[tokio::test]
async fn a_permission_request_hook_that_denies_tells_the_model_why() {
    let deny = r#"printf '%s' '{"hookSpecificOutput": {"hookEventName": "PermissionRequest", "decision": {"behavior": "deny", "message": "not on fridays"}}}'"#;
    let hooks = [(HookEvent::PermissionRequest, Dialect::Claude, deny)];
    let (exists, text, events) = delete_hooked(&hooks, Some(user()), &["y"]).await;
    assert!(exists, "{text}");
    assert!(text.contains("not on fridays"), "{text}");
    assert!(!asked(&events), "{events:?}");
}

#[cfg(unix)]
#[tokio::test]
async fn a_permission_request_hook_that_exits_two_leaves_the_prompt_open() {
    let hooks = [(
        HookEvent::PermissionRequest,
        Dialect::Claude,
        "echo no >&2; exit 2",
    )];
    let (exists, text, events) = delete_hooked(&hooks, Some(user()), &["y"]).await;
    assert!(!exists, "{text}");
    assert!(asked(&events), "{events:?}");
}

#[cfg(unix)]
#[tokio::test]
async fn the_approval_prompt_sends_a_permission_prompt_notification() {
    let seen = tempfile::NamedTempFile::new().unwrap();
    let record = format!("cat > '{}'", seen.path().display());
    let hooks = [(HookEvent::Notification, Dialect::Claude, record.as_str())];
    let (exists, text, events) = delete_hooked(&hooks, Some(user()), &["n"]).await;
    assert!(exists, "{text}");
    assert!(asked(&events), "{events:?}");
    let input: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(seen.path()).unwrap()).unwrap();
    assert_eq!(input["hook_event_name"], "Notification", "{input}");
    assert_eq!(input["notification_type"], "permission_prompt", "{input}");
    assert!(
        input["message"]
            .as_str()
            .is_some_and(|text| text.contains(DELETE)),
        "{input}"
    );
}
