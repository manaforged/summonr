use std::os::unix::fs::PermissionsExt;

use super::super::{Client, CompletionRequest, Message, Partial, Provider, Role, StopReason};
use crate::provider::{ToolCall, ToolFunction, ToolKind};

const STUB: &str = r#"#!/bin/sh
log="$0.log"
printf '%s\n' "$@" > "$0.args"
say() { printf '%s\n' "$1"; }
take() { IFS= read -r line; printf '%s\n' "$line" >> "$log"; }
mcp() { say "{\"type\":\"control_request\",\"request_id\":\"$1\",\"request\":{\"subtype\":\"mcp_message\",\"server_name\":\"summonr\",\"message\":$2}}"; }
take
mcp m0 '{"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{}}}'
take
mcp m1 '{"jsonrpc":"2.0","method":"notifications/initialized"}'
take
mcp m2 '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
take
say '{"type":"control_response","response":{"subtype":"success","request_id":"init","response":{"models":[]}}}'
take
say '{"type":"system","subtype":"init","session_id":"s"}'
say '{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"read"}}}'
say '{"type":"assistant","message":{"id":"msg_1","usage":{"input_tokens":2,"cache_read_input_tokens":40,"cache_creation_input_tokens":7,"output_tokens":1},"model":"m","content":[{"type":"text","text":"reading"},{"type":"tool_use","id":"toolu_1","name":"mcp__summonr__read","input":{"path":"a.txt"}}]}}'
say '{"type":"control_request","request_id":"p1","request":{"subtype":"can_use_tool","tool_name":"mcp__summonr__read","mcp_server":{"name":"summonr","source":"sdk"},"input":{"path":"a.txt"},"tool_use_id":"toolu_1"}}'
take
mcp m3 '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"read","arguments":{"path":"a.txt"},"_meta":{"claudecode/toolUseId":"toolu_1"}}}'
take
say '{"type":"assistant","message":{"id":"msg_2","usage":{"input_tokens":1,"cache_read_input_tokens":50,"output_tokens":1},"model":"m","content":[{"type":"text","text":"done"}]}}'
say '{"type":"result","subtype":"success","is_error":false,"result":"done","usage":{"input_tokens":3,"cache_read_input_tokens":90,"cache_creation_input_tokens":7,"output_tokens":5}}'
take
"#;

pub(super) fn read_tool() -> crate::provider::ToolDefinition {
    crate::tools::tool(
        "read",
        "Read a file.",
        serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
    )
}

pub(super) fn ask<'a>(
    messages: &'a [Message],
    tools: &'a [crate::provider::ToolDefinition],
) -> CompletionRequest<'a> {
    CompletionRequest::new(
        "s1".to_string(),
        "claude-opus-5-5".to_string(),
        messages,
        tools,
        None,
        true,
        None,
    )
}

const PLAIN: &str = r#"#!/bin/sh
{ printf '%s\n' "$@"; echo ===; } >> "$0.args"
take() { IFS= read -r line; printf '%s\n' "$line" >> "$0.log"; }
take
printf '%s\n' '{"type":"control_response","response":{"subtype":"success","request_id":"init","response":{}}}'
take
printf '%s\n' '{"type":"assistant","message":{"id":"m","content":[{"type":"text","text":"ok"}]}}'
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"ok","usage":{}}'
IFS= read -r line
"#;

pub(super) fn seat_at(dir: &std::path::Path, script: &str) -> Client {
    let stub = dir.join("claude");
    std::fs::write(&stub, script).unwrap();
    std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut seat =
        crate::endpoint::resolve(Some("claude"), Some("claude-opus-5-5"), crate::Need::Config)
            .unwrap();
    seat.base_url = Some(stub.display().to_string());
    Client::connect(&seat).unwrap()
}

pub(super) fn said(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("claude.log"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .filter(|sent| sent["type"] == "user")
        .map(|sent| {
            sent["message"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_fresh_claude_process_gets_the_earlier_turns_and_the_next_turn_resumes_it() {
    let dir = tempfile::tempdir().unwrap();
    let client = seat_at(dir.path(), PLAIN);
    let mut earlier = Message::user("earlier answer");
    earlier.role = Role::Assistant;
    let mut messages = vec![
        Message::system("sys"),
        Message::user("first question"),
        earlier,
        Message::user("next question"),
    ];
    client
        .stream(&ask(&messages, &[]), &mut Partial::default())
        .await
        .unwrap();
    let mut answered = Message::user("ok");
    answered.role = Role::Assistant;
    messages.push(answered);
    messages.push(Message::user("again"));
    client
        .stream(&ask(&messages, &[]), &mut Partial::default())
        .await
        .unwrap();

    let sent = said(dir.path());
    assert_eq!(sent.len(), 2, "{sent:?}");
    assert!(sent[0].starts_with("<earlier_conversation>"), "{}", sent[0]);
    assert!(sent[0].contains("[user] first question"), "{}", sent[0]);
    assert!(
        sent[0].contains("[assistant] earlier answer"),
        "{}",
        sent[0]
    );
    assert!(sent[0].ends_with("next question"), "{}", sent[0]);
    assert_eq!(sent[1], "again");
    let args = std::fs::read_to_string(dir.path().join("claude.args")).unwrap();
    let runs: Vec<Vec<&str>> = args
        .split("===\n")
        .filter(|block| !block.is_empty())
        .map(|block| block.lines().collect())
        .collect();
    let value = |run: &Vec<&str>, flag: &str| {
        run.windows(2)
            .find(|window| window[0] == flag)
            .map(|window| window[1].to_string())
    };
    let started = value(&runs[0], "--session-id").expect("first run starts a session");
    assert_eq!(value(&runs[1], "--resume"), Some(started));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plan_turn_runs_summonr_tools_through_the_sdk_bridge() {
    let dir = tempfile::tempdir().unwrap();
    let client = seat_at(dir.path(), STUB);
    let tools = [read_tool()];
    let mut messages = vec![Message::system("sys"), Message::user("read a.txt")];

    let first = client
        .stream(&ask(&messages, &tools), &mut Partial::default())
        .await
        .unwrap();
    assert_eq!(first.stop_reason, StopReason::ToolCalls);
    assert_eq!(first.content.as_deref(), Some("reading"));
    assert_eq!(first.tool_calls.len(), 1, "{:?}", first.tool_calls);
    assert_eq!(first.tool_calls[0].id, "toolu_1");
    assert_eq!(first.usage.prompt_tokens, 49);
    assert_eq!(first.usage.cached_tokens, 40);
    assert_eq!(first.tool_calls[0].function.name, "read");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&first.tool_calls[0].function.arguments).unwrap(),
        serde_json::json!({"path": "a.txt"})
    );

    let mut assistant = Message::user("reading");
    assistant.role = Role::Assistant;
    assistant.tool_calls = vec![ToolCall {
        id: "toolu_1".to_string(),
        kind: ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "read".to_string(),
            arguments: "{\"path\":\"a.txt\"}".to_string(),
        },
        continuation: None,
    }];
    let mut result = Message::user("1:file text");
    result.role = Role::Tool;
    result.tool_call_id = Some("toolu_1".to_string());
    messages.push(assistant);
    messages.push(result);

    let last = client
        .stream(&ask(&messages, &tools), &mut Partial::default())
        .await
        .unwrap();
    assert_eq!(last.stop_reason, StopReason::Stop);
    assert_eq!(last.content.as_deref(), Some("done"));
    assert!(last.tool_calls.is_empty());
    assert_eq!(last.usage.prompt_tokens, 51);
    assert_eq!(last.usage.cached_tokens, 50);
    assert_eq!(last.usage.cache_write_tokens, 0);
    assert_eq!(
        first.usage.completion_tokens + last.usage.completion_tokens,
        5
    );

    let args = std::fs::read_to_string(dir.path().join("claude.args")).unwrap();
    let args: Vec<&str> = args.lines().collect();
    for pair in [
        ["--tools", ""],
        ["--permission-prompt-tool", "stdio"],
        ["--model", "claude-opus-5-5"],
        ["--input-format", "stream-json"],
    ] {
        assert!(
            args.windows(2).any(|window| window == pair),
            "{pair:?} in {args:?}"
        );
    }
    let sent: Vec<serde_json::Value> = std::fs::read_to_string(dir.path().join("claude.log"))
        .unwrap()
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(sent[0]["request"]["subtype"], "initialize");
    assert_eq!(sent[0]["request_id"], "init");
    let listed = &sent[3]["response"]["response"]["mcp_response"]["result"]["tools"][0];
    assert_eq!(listed["name"], "read");
    assert_eq!(
        listed["inputSchema"]["properties"]["path"]["type"],
        "string"
    );
    assert_eq!(sent[4]["type"], "user");
    assert_eq!(sent[4]["message"]["content"][0]["text"], "read a.txt");
    assert_eq!(sent[5]["response"]["response"]["behavior"], "allow");
    assert_eq!(sent[5]["response"]["request_id"], "p1");
    let called = &sent[6]["response"]["response"]["mcp_response"];
    assert_eq!(called["id"], 2);
    assert_eq!(called["result"]["content"][0]["text"], "1:file text");
    assert_eq!(called["result"]["isError"], false);
}

#[path = "agent_tests/recover_tests.rs"]
mod recover;

fn runs(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(dir.join("claude.args"))
        .unwrap_or_default()
        .split("===\n")
        .filter(|block| !block.is_empty())
        .map(str::to_string)
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn claude_code_never_sees_an_api_key() {
    let unset = crate::endpoint::env_unset("claude");
    for key in ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"] {
        assert!(unset.contains(&key), "{key} reaches Claude Code");
    }
    let cwd = std::env::current_dir().unwrap();
    let mut process = crate::rpc::start(&crate::rpc::Launch {
        program: "sh",
        args: &["-c", "printf %s \"${HOME-absent}\""],
        env: &[],
        unset: &["HOME"],
        cwd: &cwd,
        sandbox: None,
        base: crate::rpc::Env::Inherit,
        stderr: crate::rpc::Stderr::Discard,
    })
    .unwrap();
    let mut seen = String::new();
    tokio::io::AsyncReadExt::read_to_string(&mut process.stdout, &mut seen)
        .await
        .unwrap();
    assert_eq!(seen, "absent", "an unset variable was inherited");
}

const SECOND_HANGS: &str = r#"#!/bin/sh
n=$(cat "$0.n" 2>/dev/null || echo 0); n=$((n+1)); echo "$n" > "$0.n"
{ printf '%s\n' "$@"; echo ===; } >> "$0.args"
take() { IFS= read -r line; printf '%s\n' "$line" >> "$0.log"; }
take
printf '%s\n' '{"type":"control_response","response":{"subtype":"success","request_id":"init","response":{}}}'
take
if [ "$n" = 2 ]; then sleep 20; fi
printf '%s\n' '{"type":"assistant","message":{"id":"m","content":[{"type":"text","text":"ok"}]}}'
printf '%s\n' '{"type":"result","subtype":"success","is_error":false,"result":"ok","usage":{}}'
IFS= read -r line
"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_turn_forgets_the_claude_thread() {
    let dir = tempfile::tempdir().unwrap();
    let client = seat_at(dir.path(), SECOND_HANGS);
    let mut messages = vec![Message::system("sys"), Message::user("first")];
    client
        .stream(&ask(&messages, &[]), &mut Partial::default())
        .await
        .unwrap();
    let mut answered = Message::user("ok");
    answered.role = Role::Assistant;
    messages.push(answered);
    messages.push(Message::user("second"));
    let cancelled = tokio::time::timeout(
        std::time::Duration::from_millis(800),
        client.stream(&ask(&messages, &[]), &mut Partial::default()),
    )
    .await;
    assert!(cancelled.is_err(), "the hanging turn finished");
    messages.push(Message::user("third"));
    client
        .stream(&ask(&messages, &[]), &mut Partial::default())
        .await
        .unwrap();
    let runs = runs(dir.path());
    assert_eq!(runs.len(), 3, "{runs:?}");
    assert!(runs[1].contains("--resume"), "{}", runs[1]);
    assert!(
        !runs[2].contains("--resume"),
        "the cancelled thread was resumed: {}",
        runs[2]
    );
}

const MALFORMED: &str = r#"#!/bin/sh
take() { IFS= read -r line; printf '%s\n' "$line" >> "$0.log"; }
take
printf '%s\n' '{"type":"control_response","response":{"subtype":"success","request_id":"init","response":{}}}'
take
printf '%s\n' '{"type":"control_request","request_id":"x","request":{"subtype":"mcp_message","server_name":"summonr"}}'
sleep 20
"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_malformed_control_request_fails_the_turn() {
    let dir = tempfile::tempdir().unwrap();
    let client = seat_at(dir.path(), MALFORMED);
    let messages = vec![Message::system("sys"), Message::user("hi")];
    let turn = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        client.stream(&ask(&messages, &[]), &mut Partial::default()),
    )
    .await
    .expect("the turn waited on an unanswerable request");
    let error = turn.expect_err("a malformed control request passed");
    assert!(error.to_string().contains("mcp_message"), "{error}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rewritten_history_starts_a_fresh_thread() {
    let dir = tempfile::tempdir().unwrap();
    let client = seat_at(dir.path(), PLAIN);
    let mut first = vec![Message::system("sys"), Message::user("long history")];
    client
        .stream(&ask(&first, &[]), &mut Partial::default())
        .await
        .unwrap();
    first.truncate(1);
    first.push(Message::user("summary of the work so far"));
    first.push(Message::user("next"));
    client
        .stream(&ask(&first, &[]), &mut Partial::default())
        .await
        .unwrap();
    let runs = runs(dir.path());
    assert_eq!(runs.len(), 2, "{runs:?}");
    assert!(
        !runs[1].contains("--resume"),
        "a compacted history resumed the old thread: {}",
        runs[1]
    );
    let sent = said(dir.path());
    assert!(
        sent[1].contains("summary of the work so far"),
        "{}",
        sent[1]
    );
}
