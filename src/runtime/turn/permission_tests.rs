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
            arguments: serde_json::json!({
                "command": text,
                "timeout_secs": crate::limits::limits().command_kill_secs,
            })
            .to_string(),
        },
        continuation: None,
    }
}

async fn delete_outside(
    steering: Option<Arc<Steering>>,
    answers: &[&str],
) -> (bool, String, Vec<Event>) {
    let workspace = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let target = workspace.path().to_path_buf();
    let mut loop_ = session(workspace.path(), home.path()).with_unrestricted_commands();
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
        .with_grant(output, &call, &mut events, &mut reply)
        .await
        .unwrap()
        .unwrap();
    (target.exists(), output.text, events)
}

fn user() -> Arc<Steering> {
    Arc::new(Steering::default())
}

#[cfg(unix)]
#[tokio::test]
async fn steering_answer_never_comes_from_a_steer_waiting_in_the_queue() {
    let steering = user();
    steering
        .lock()
        .unwrap()
        .push_back(UserInput::from("also update the docs"));
    let (exists, text, _) = delete_outside(Some(Arc::clone(&steering)), &["y"]).await;
    assert!(!exists, "{text}");
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

#[tokio::test]
async fn a_destructive_command_never_runs_without_a_user() {
    let (exists, text, _) = delete_outside(None, &[]).await;
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
    let steering = user();
    loop_.steer_with(Arc::clone(&steering));
    let mut reply = steering.replying(&["y", "n"]);
    let call = command(&format!(
        "{DELETE} ./*; python3 -c \"from pathlib import Path; Path('{}').unlink()\"",
        target.join("sentinel").display()
    ));
    let output = loop_.tools.execute_outcome(&call).await;
    let mut events = Vec::new();

    let output = loop_
        .with_grant(output, &call, &mut events, &mut reply)
        .await??;

    assert!(
        events.iter().any(|event| matches!(event,
            Event::Approval { detail: Some(detail), .. } if detail.contains(DELETE)
        )),
        "the command never requested approval: {events:?}"
    );
    assert_eq!(std::fs::read_to_string(target.join("sentinel"))?, "keep\n");
    assert!(
        !output.success,
        "the outside deletion escaped containment: {}",
        output.text
    );
    Ok(())
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
