use super::super::super::{Message, Partial, Provider, Role, StopReason};
use super::{ask, client_at, read_tool, said};
use crate::provider::{ToolCall, ToolFunction, ToolKind};

const RUNS: &str = "claude/recover";

async fn turn(
    client: &super::super::super::Client,
    messages: &[Message],
    tools: &[crate::provider::ToolDefinition],
) -> summonr_core::error::Result<crate::provider::AssistantMessage> {
    let request = ask(messages, tools);
    let mut partial = Partial::default();
    tokio::time::timeout(
        std::time::Duration::from_secs(20),
        client.stream(&request, &mut partial),
    )
    .await
    .expect("the turn finished instead of waiting on claude")
}

fn mode(dir: &std::path::Path, mode: &str) {
    std::fs::write(dir.join("claude.mode"), mode).unwrap();
}

fn runs(dir: &std::path::Path) -> Vec<Vec<String>> {
    std::fs::read_to_string(dir.join("claude.args"))
        .unwrap()
        .split("===\n")
        .filter(|block| !block.is_empty())
        .map(|block| block.lines().map(str::to_string).collect())
        .collect()
}

fn flag(run: &[String], name: &str) -> Option<String> {
    run.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].clone())
}

fn assistant(text: &str, calls: Vec<ToolCall>) -> Message {
    let mut message = Message::user(text);
    message.role = Role::Assistant;
    message.tool_calls = calls;
    message
}

fn call(id: &str) -> ToolCall {
    ToolCall {
        id: id.to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "read".to_string(),
            arguments: "{}".to_string(),
        },
        continuation: None,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_prompt_after_an_abandoned_tool_call_starts_a_fresh_resumed_run() {
    let dir = tempfile::tempdir().unwrap();
    mode(dir.path(), "tool");
    let client = client_at(dir.path(), RUNS);
    let tools = [read_tool()];
    let mut messages = vec![Message::system("sys"), Message::user("read")];
    let paused = turn(&client, &messages, &tools).await.unwrap();
    assert_eq!(paused.stop_reason, StopReason::ToolCalls);
    messages.push(assistant("", vec![call("toolu_9")]));
    messages.push(Message::user("never mind, say hi"));
    let reply = turn(&client, &messages, &tools).await.unwrap();
    assert_eq!(reply.content.as_deref(), Some("run 2"));
    let runs = runs(dir.path());
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert_eq!(flag(&runs[1], "--resume"), flag(&runs[0], "--session-id"));
    assert_eq!(
        said(dir.path()).last().map(String::as_str),
        Some("never mind, say hi")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_resumed_run_after_tool_results_gets_those_results_as_input() {
    let dir = tempfile::tempdir().unwrap();
    mode(dir.path(), "plain");
    let client = client_at(dir.path(), RUNS);
    let mut messages = vec![Message::system("sys"), Message::user("first")];
    turn(&client, &messages, &[]).await.unwrap();
    messages.push(assistant("run 1", Vec::new()));
    messages.push(Message::user("again"));
    messages.push(assistant("", vec![call("toolu_5")]));
    let mut result = Message::user("1:file text");
    result.role = Role::Tool;
    result.tool_call_id = Some("toolu_5".to_string());
    messages.push(result);
    turn(&client, &messages, &[]).await.unwrap();
    let sent = said(dir.path());
    assert!(
        sent.last().is_some_and(|text| text.contains("1:file text")),
        "{sent:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_turn_starts_the_next_turn_in_a_new_session() {
    let dir = tempfile::tempdir().unwrap();
    mode(dir.path(), "fail");
    let client = client_at(dir.path(), RUNS);
    let mut messages = vec![Message::system("sys"), Message::user("first")];
    let failed = turn(&client, &messages, &[]).await.unwrap_err();
    assert!(failed.to_string().contains("boom"), "{failed}");
    messages.push(Message::user("retry"));
    turn(&client, &messages, &[]).await.unwrap();
    let runs = runs(dir.path());
    assert!(flag(&runs[1], "--resume").is_none(), "{runs:?}");
    assert!(
        flag(&runs[1], "--session-id").is_some_and(|id| Some(id) != flag(&runs[0], "--session-id"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn claude_code_gets_the_users_environment_and_the_prompt_in_a_file() {
    let dir = tempfile::tempdir().unwrap();
    mode(dir.path(), "plain");
    let client = client_at(dir.path(), RUNS);
    let messages = [Message::system("the system prompt"), Message::user("hi")];
    turn(&client, &messages, &[]).await.unwrap();
    let env = std::fs::read_to_string(dir.path().join("claude.env")).unwrap();
    assert!(
        !env.trim().is_empty(),
        "the parent environment reaches claude"
    );
    let prompt = std::fs::read_to_string(dir.path().join("claude.prompt")).unwrap();
    assert_eq!(prompt, "the system prompt");
    assert!(flag(&runs(dir.path())[0], "--system-prompt").is_none());
    assert_eq!(
        flag(&runs(dir.path())[0], "--system-prompt-snapshot").as_deref(),
        Some("off")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_built_in_claude_code_tool_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    mode(dir.path(), "deny");
    let client = client_at(dir.path(), RUNS);
    let messages = [Message::system("sys"), Message::user("hi")];
    turn(&client, &messages, &[]).await.unwrap();
    let log = std::fs::read_to_string(dir.path().join("claude.log")).unwrap();
    let reply: serde_json::Value = log
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|sent| sent["response"]["request_id"] == "p7")
        .unwrap();
    assert_eq!(reply["response"]["response"]["behavior"], "deny");
}

#[tokio::test(flavor = "multi_thread")]
async fn two_turns_on_one_session_run_one_after_the_other() {
    let dir = tempfile::tempdir().unwrap();
    mode(dir.path(), "plain");
    let client = client_at(dir.path(), RUNS);
    let messages = [Message::system("sys"), Message::user("hi")];
    let (left, right) = tokio::join!(turn(&client, &messages, &[]), turn(&client, &messages, &[]));
    left.unwrap();
    right.unwrap();
    let runs = runs(dir.path());
    let started = flag(&runs[0], "--session-id");
    assert!(started.is_some(), "{runs:?}");
    assert_eq!(flag(&runs[1], "--resume"), started, "{runs:?}");
}
