use super::*;
use crate::provider::{Client, ToolFunction, ToolKind};

fn session(workspace: &Path, home: &Path) -> Loop<Client> {
    let client = Client::new("k", "http://127.0.0.1:9/v1").unwrap();
    let driver = super::super::configure::open_driver(home, workspace, "m", "system").unwrap();
    Loop::open(client, "m", workspace, "system", driver).unwrap()
}

fn command(text: &str) -> ToolCall {
    ToolCall {
        id: "t1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            name: "run_command".to_string(),
            arguments: serde_json::json!({ "command": text }).to_string(),
        },
        continuation: None,
    }
}

async fn delete_outside(answer: Option<&str>) -> (bool, String, Vec<Event>) {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("victim");
    std::fs::create_dir(&target).unwrap();
    let mut loop_ = session(workspace.path(), home.path()).with_unrestricted_commands();
    if let Some(answer) = answer {
        let queue = VecDeque::from([UserInput::from(answer.to_string())]);
        loop_.steer_with(Arc::new(Mutex::new(queue)));
    }
    let call = command(&format!("rm -rf {}", target.display()));
    let output = loop_.tools.execute_outcome(&call).await;
    let mut events = Vec::new();
    let output = loop_
        .with_grant(output, &call, &mut events, &mut |_| {})
        .await
        .unwrap()
        .unwrap();
    (target.exists(), output.text, events)
}

#[tokio::test]
async fn a_destructive_command_asks_the_user_first() {
    let (exists, text, events) = delete_outside(Some("n")).await;
    assert!(exists, "{text}");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Approval { detail: Some(detail), .. } if detail.contains("rm -rf"))),
        "{events:?}"
    );
}

#[tokio::test]
async fn a_confirmed_destructive_command_runs() {
    let (exists, text, _) = delete_outside(Some("y")).await;
    assert!(!exists, "{text}");
}

#[tokio::test]
async fn a_destructive_command_never_runs_without_a_user() {
    let (exists, text, _) = delete_outside(None).await;
    assert!(exists, "{text}");
}

#[cfg(target_os = "macos")]
#[test]
fn leaving_plan_drops_the_sandbox_plan_turned_on() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut loop_ = session(workspace.path(), home.path()).with_plan();
    assert!(loop_.has_sandbox());
    loop_.set_mode(crate::RunMode::Auto).unwrap();
    assert!(!loop_.has_sandbox());
}

#[cfg(target_os = "macos")]
#[test]
fn leaving_plan_keeps_a_sandbox_the_user_asked_for() {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let mut loop_ = session(workspace.path(), home.path())
        .with_sandbox(false)
        .unwrap()
        .with_plan();
    loop_.set_mode(crate::RunMode::Auto).unwrap();
    assert!(loop_.has_sandbox());
}
