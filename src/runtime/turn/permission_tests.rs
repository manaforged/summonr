use super::*;
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
            arguments: serde_json::json!({ "command": text }).to_string(),
        },
        continuation: None,
    }
}

async fn delete_outside(answer: Option<&str>) -> (bool, String, Vec<Event>) {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let target = workspace.path().to_path_buf();
    let mut loop_ = session(workspace.path(), home.path()).with_unrestricted_commands();
    if let Some(answer) = answer {
        let queue = VecDeque::from([UserInput::from(answer.to_string())]);
        loop_.steer_with(Arc::new(Steering::from(queue)));
    }
    let call = command(&format!("{DELETE} {}", target.display()));
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
            .any(|event| matches!(event, Event::Approval { detail: Some(detail), .. } if detail.contains(DELETE))),
        "{events:?}"
    );
}

#[cfg(unix)]
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

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[tokio::test]
async fn approving_a_destructive_command_keeps_the_sandbox() -> Result<()> {
    let workspace = tempfile::tempdir()?;
    let home = tempfile::tempdir()?;
    let outside = tempfile::tempdir_in(std::env::current_dir()?)?;
    let target = outside.path().join("keep");
    std::fs::create_dir(&target)?;
    std::fs::write(target.join("sentinel"), "keep\n")?;
    let mut loop_ = session(workspace.path(), home.path()).with_sandbox(crate::Network::Allowed)?;
    let answers = VecDeque::from([UserInput::from("y"), UserInput::from("n")]);
    loop_.steer_with(Arc::new(Steering::from(answers)));
    let call = command(&format!(
        "{DELETE} ./*; python3 -c \"from pathlib import Path; Path('{}').unlink()\"",
        target.join("sentinel").display()
    ));
    let output = loop_.tools.execute_outcome(&call).await;
    let mut events = Vec::new();

    let output = loop_
        .with_grant(output, &call, &mut events, &mut |_| {})
        .await??;

    assert!(
        events.iter().any(|event| matches!(event,
            Event::Approval { detail: Some(detail), .. } if detail.contains(DELETE)
        )),
        "the command never requested approval: {events:?}"
    );
    assert_eq!(std::fs::read_to_string(target.join("sentinel"))?, "keep\n");
    assert!(!output.success, "the outside deletion escaped containment");
    Ok(())
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
        .with_sandbox(crate::Network::Allowed)
        .unwrap()
        .with_plan();
    loop_.set_mode(crate::RunMode::Auto).unwrap();
    assert!(loop_.has_sandbox());
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
            )
            .await;
        assert_eq!(text, "exit 2 make: *** No rule to make target");
    }
}
