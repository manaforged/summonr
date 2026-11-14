use super::super::{Client, CompletionRequest, Message, Partial, Provider, Role, StopReason};
use crate::provider::{ToolCall, ToolFunction, ToolKind};

#[path = "agent_tests/auth_tests.rs"]
mod auth_tests;

const STUB: &str = "claude/stream";

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

pub(super) const PLAIN: &str = "claude/plain";

pub(super) fn client_at(dir: &std::path::Path, fixture: &str) -> Client {
    let stub = crate::fixture_tests::link(fixture, &dir.join("claude"));
    let mut selection = crate::endpoint::resolve(
        Some("claude"),
        Some("claude-opus-5-5"),
        crate::Requirement::Config,
    )
    .unwrap();
    selection.base_url = Some(stub.display().to_string());
    Client::connect(&selection).unwrap()
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

pub(super) fn reply(messages: &mut Vec<Message>, next: &str) {
    let mut answered = Message::user("ok");
    answered.role = Role::Assistant;
    messages.push(answered);
    messages.push(Message::user(next));
}

#[tokio::test(flavor = "multi_thread")]
async fn two_turns_share_one_claude_process() {
    let dir = tempfile::tempdir().unwrap();
    let client = client_at(dir.path(), PLAIN);
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
    reply(&mut messages, "again");
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
    let runs = runs(dir.path());
    assert_eq!(
        runs.len(),
        1,
        "the second turn started claude again: {runs:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_plan_turn_runs_summonr_tools_through_the_sdk_bridge() {
    let dir = tempfile::tempdir().unwrap();
    let client = client_at(dir.path(), STUB);
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

#[path = "agent_tests/held_tests.rs"]
mod held;

pub(super) fn runs(dir: &std::path::Path) -> Vec<String> {
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
        assert!(
            unset.iter().any(|name| name == key),
            "{key} reaches Claude Code"
        );
    }
    let cwd = std::env::current_dir().unwrap();
    let mut process = crate::process::start(crate::process::Spec {
        remove: vec!["HOME".into()],
        cwd: Some(cwd),
        stderr: crate::process::Io::Null,
        ..crate::process::Spec::new("sh", ["-c", "printf %s \"${HOME-absent}\""])
    })
    .unwrap();
    let mut seen = String::new();
    tokio::io::AsyncReadExt::read_to_string(&mut process.stdout, &mut seen)
        .await
        .unwrap();
    assert_eq!(seen, "absent", "an unset variable was inherited");
}

const MALFORMED: &str = "claude/malformed";

#[tokio::test(flavor = "multi_thread")]
async fn a_malformed_control_request_fails_the_turn() {
    let dir = tempfile::tempdir().unwrap();
    let client = client_at(dir.path(), MALFORMED);
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
    let client = client_at(dir.path(), PLAIN);
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
