use super::super::super::{Message, Partial, Provider, Role, StopReason};
use super::{ask, read_tool, said, seat_at};
use crate::provider::{ToolCall, ToolFunction, ToolKind};

const RUNS: &str = r#"#!/bin/sh
n=$(cat "$0.n" 2>/dev/null || echo 0); n=$((n+1)); echo "$n" > "$0.n"
{ printf '%s\n' "$@"; echo ===; } >> "$0.args"
printf '%s\n' "$CARGO_MANIFEST_DIR" > "$0.env"
prev=""; for a in "$@"; do if [ "$prev" = "--system-prompt-file" ]; then cat "$a" > "$0.prompt"; fi; prev="$a"; done
say() { printf '%s\n' "$1"; }
take() { IFS= read -r line; printf '%s\n' "$line" >> "$0.log"; }
take
say '{"type":"control_response","response":{"subtype":"success","request_id":"init","response":{}}}'
take
if [ "$MODE" = "" ]; then MODE=$(cat "$0.mode"); fi
case "$MODE$n" in
  tool1)
    say '{"type":"assistant","message":{"id":"a","content":[{"type":"tool_use","id":"toolu_9","name":"mcp__summonr__read","input":{}}]}}'
    say '{"type":"control_request","request_id":"m9","request":{"subtype":"mcp_message","server_name":"summonr","message":{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"read","arguments":{},"_meta":{"claudecode/toolUseId":"toolu_9"}}}}}'
    IFS= read -r line
    exit 0 ;;
  fail1)
    say '{"type":"assistant","message":{"id":"a","content":[{"type":"text","text":"x"}]}}'
    say '{"type":"result","subtype":"error_during_execution","is_error":true,"errors":["boom"],"usage":{}}'
    IFS= read -r line
    exit 0 ;;
esac
say "{\"type\":\"assistant\",\"message\":{\"id\":\"b$n\",\"content\":[{\"type\":\"text\",\"text\":\"run $n\"}]}}"
say '{"type":"result","subtype":"success","is_error":false,"result":"done","usage":{}}'
IFS= read -r line
"#;

async fn turn(
    client: &super::super::super::Client,
    messages: &[Message],
    tools: &[crate::provider::ToolDefinition],
) -> crate::error::Result<crate::provider::AssistantMessage> {
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
    let client = seat_at(dir.path(), RUNS);
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
    let client = seat_at(dir.path(), RUNS);
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
    let client = seat_at(dir.path(), RUNS);
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
    let client = seat_at(dir.path(), RUNS);
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
}
