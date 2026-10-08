use std::io::{Read, Write};

use serde_json::{Value, json};

use super::{Client, CompletionRequest, Effort, Message, Partial, Role, ToolCall, ToolFunction};
use crate::endpoint::{CacheTtl, Endpoint};

fn endpoint(name: &str, model: &str) -> Endpoint {
    let mut endpoint =
        crate::endpoint::resolve(Some(name), Some(model), crate::Need::Config).unwrap();
    endpoint.api_key = Some("k".to_string());
    endpoint
}

fn tool_round() -> Vec<Message> {
    let mut assistant = Message::user("");
    assistant.role = Role::Assistant;
    assistant.tool_calls = vec![ToolCall {
        id: "toolu_1".to_string(),
        kind: super::ToolKind::Function,
        function: ToolFunction {
            checked: Default::default(),
            name: "read".to_string(),
            arguments: "{}".to_string(),
        },
        continuation: None,
    }];
    let mut result = Message::user("file text");
    result.role = Role::Tool;
    result.tool_call_id = Some("toolu_1".to_string());
    vec![
        Message::system("sys"),
        Message::user("read it"),
        assistant,
        result,
    ]
}

fn body(endpoint: &Endpoint, messages: &[Message], effort: Option<Effort>) -> Value {
    let request = CompletionRequest::new(
        "s".to_string(),
        endpoint.model.clone(),
        messages,
        &[],
        Some(16_000),
        false,
        effort,
    );
    Client::connect(endpoint)
        .unwrap()
        .request_body(&request, false)
        .unwrap()
}

#[test]
fn adaptive_thinking_stays_on_after_a_tool_round_without_thinking() {
    let anthropic = endpoint("anthropic", "claude-opus-5");
    let body = body(&anthropic, &tool_round(), Some(Effort::High));
    assert_eq!(body["thinking"]["type"], "adaptive", "{body}");
}

#[test]
fn every_anthropic_breakpoint_carries_the_endpoint_ttl() {
    let mut anthropic = endpoint("anthropic", "claude-opus-5");
    let long = body(&anthropic, &tool_round(), Some(Effort::High));
    let hour = json!({"type": "ephemeral", "ttl": "1h"});
    let system = long["system"].as_array().unwrap();
    assert_eq!(system.last().unwrap()["cache_control"], hour);
    let last = long["messages"].as_array().unwrap().last().unwrap()["content"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .clone();
    assert_eq!(last["cache_control"], hour);
    anthropic.cache_ttl = CacheTtl::FiveMinutes;
    let short = body(&anthropic, &tool_round(), Some(Effort::High));
    let system = short["system"].as_array().unwrap();
    assert_eq!(
        system.last().unwrap()["cache_control"],
        json!({"type": "ephemeral"})
    );
    assert_eq!(
        serde_json::from_str::<CacheTtl>("\"5m\"").unwrap(),
        CacheTtl::FiveMinutes
    );
}

#[test]
fn claude_through_a_completions_host_marks_its_cache_breakpoints() {
    let messages = [Message::system("sys"), Message::user("hi")];
    let claude = body(
        &endpoint("openrouter", "anthropic/claude-sonnet-4.5"),
        &messages,
        None,
    );
    assert_eq!(
        claude["messages"][0]["content"],
        json!([{"type": "text", "text": "sys", "cache_control": {"type": "ephemeral"}}])
    );
    assert_eq!(
        claude["messages"][1]["content"][0]["cache_control"],
        json!({"type": "ephemeral"})
    );
    let other = body(&endpoint("openrouter", "openai/gpt-4o"), &messages, None);
    assert_eq!(other["messages"][0]["content"], "sys");
    assert_eq!(other["messages"][1]["content"], "hi");
}

#[test]
fn grok_routes_every_request_to_its_cache_with_the_prefix_hash() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let mut grok = endpoint("grok", "grok-4.6");
    grok.base_url = Some(format!("http://{}/v1", listener.local_addr().unwrap()));
    grok.oauth = None;
    let heard = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut seen = Vec::new();
        let mut chunk = [0u8; 65536];
        loop {
            let read = socket.read(&mut chunk).unwrap();
            if read == 0 {
                break;
            }
            seen.extend_from_slice(&chunk[..read]);
            let Some(end) = seen.windows(4).position(|window| window == b"\r\n\r\n") else {
                continue;
            };
            let head = String::from_utf8_lossy(&seen[..end]).to_ascii_lowercase();
            let length = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if seen.len() >= end + 4 + length {
                break;
            }
        }
        socket
            .write_all(
                b"HTTP/1.1 400 Bad Request\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}",
            )
            .unwrap();
        String::from_utf8_lossy(&seen).to_ascii_lowercase()
    });
    let messages = [Message::system("sys"), Message::user("hi")];
    let request = CompletionRequest::new(
        "s".to_string(),
        grok.model.clone(),
        &messages,
        &[],
        Some(1_000),
        false,
        None,
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _context = runtime.enter();
    let client = Client::connect(&grok).unwrap();
    let outcome = runtime.block_on(client.complete_stream(&request, &mut Partial::default()));
    let failure = outcome.unwrap_err().to_string();
    let headers = heard.join().unwrap();
    assert!(
        !headers.is_empty(),
        "no request reached the server: {failure}"
    );
    let want = format!("x-grok-conv-id: {}", request.prefix_hash()).to_ascii_lowercase();
    assert!(headers.contains(&want), "{headers}");
}
