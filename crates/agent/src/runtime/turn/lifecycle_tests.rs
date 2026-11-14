use serde_json::{Value, json};

use super::permission_tests::{hooked, session};
use super::*;
use crate::hooks::{Dialect, Hook, HookEvent};
use crate::provider::Client;

fn append(log: &Path) -> String {
    format!("cat >> '{0}'; echo >> '{0}'", log.display())
}

fn logged(log: &Path) -> Vec<Value> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).expect("a hook logs one JSON object per line"))
        .collect()
}

fn named(seen: &[Value]) -> Vec<String> {
    seen.iter()
        .map(|event| {
            let detail = event["source"].as_str().or(event["reason"].as_str());
            format!(
                "{}:{}",
                event["hook_event_name"].as_str().unwrap_or_default(),
                detail.unwrap_or_default()
            )
        })
        .collect()
}

async fn admitted(loop_: &mut Loop<Client>, text: &str) -> String {
    let mut input = UserInput::from(text);
    loop_
        .admit(&mut input, &mut |_: &Event| {})
        .await
        .expect("the prompt is admitted");
    input.text
}

#[cfg(unix)]
#[tokio::test]
async fn session_start_runs_when_the_session_opens_and_again_after_clear() {
    let workspace = tempfile::tempdir().expect("make the workspace");
    let home = tempfile::tempdir().expect("make the home");
    let log = home.path().join("hooks.jsonl");
    let mut loop_ = session(workspace.path(), home.path());
    hooked(
        &mut loop_,
        HookEvent::SessionStart,
        Dialect::Claude,
        &append(&log),
    );
    hooked(
        &mut loop_,
        HookEvent::SessionEnd,
        Dialect::Claude,
        &append(&log),
    );
    loop_.begin().await;
    loop_.begin().await;
    let opened = named(&logged(&log));
    assert_eq!(opened, ["SessionStart:startup"]);
    loop_.clear().await.expect("clear the history");
    let cleared = named(&logged(&log));
    assert_eq!(
        cleared,
        [
            "SessionStart:startup",
            "SessionEnd:clear",
            "SessionStart:clear"
        ]
    );
}

#[cfg(unix)]
#[tokio::test]
async fn session_start_context_waits_for_the_first_prompt_only() {
    let workspace = tempfile::tempdir().expect("make the workspace");
    let home = tempfile::tempdir().expect("make the home");
    let mut loop_ = session(workspace.path(), home.path());
    let context = json!({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": "the codename is heron"}});
    let script = format!("printf '%s' '{context}'");
    hooked(
        &mut loop_,
        HookEvent::SessionStart,
        Dialect::Claude,
        &script,
    );
    loop_.begin().await;
    assert!(
        admitted(&mut loop_, "go")
            .await
            .contains("the codename is heron")
    );
    assert!(!admitted(&mut loop_, "again").await.contains("heron"));
}

#[cfg(unix)]
#[tokio::test]
async fn an_async_hook_does_not_hold_the_call_and_its_context_reaches_a_later_prompt() {
    let workspace = tempfile::tempdir().expect("make the workspace");
    let home = tempfile::tempdir().expect("make the home");
    let gate = home.path().join("gate");
    let mut loop_ = session(workspace.path(), home.path());
    let context = json!({"hookSpecificOutput": {"hookEventName": "PostToolUse", "additionalContext": "the tests passed"}});
    let made = std::process::Command::new("mkfifo")
        .arg(&gate)
        .status()
        .expect("run mkfifo");
    assert!(made.success(), "mkfifo makes the gate");
    let script = format!("read -r _ < '{}'; printf '%s' '{context}'", gate.display());
    let mut hook = Hook::plain(script, Dialect::Claude);
    hook.background = true;
    loop_.hooks.push(HookEvent::PostTool, hook);
    let payload = json!({ "tool": "run_command", "arguments": {} });
    let heard = crate::limits::within(
        Duration::from_secs(5),
        "the async hook call",
        loop_.hook(HookEvent::PostTool, Some("run_command"), payload),
    )
    .await
    .expect("an async hook returns before its command exits");
    assert!(heard.context.is_empty());
    tokio::fs::write(&gate, "open\n")
        .await
        .expect("open the gate");
    crate::limits::within(
        Duration::from_secs(5),
        "the async hook command",
        loop_.pending_hooks.settled(),
    )
    .await
    .expect("the async hook exits once the gate opens");
    assert!(
        admitted(&mut loop_, "next")
            .await
            .contains("the tests passed"),
        "the async hook's context reaches the next prompt"
    );
}
