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

fn sent(mut endpoint: Endpoint, request: &CompletionRequest<'_>) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    endpoint.base_url = Some(format!("http://{}/v1", listener.local_addr().unwrap()));
    endpoint.oauth = None;
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
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _context = runtime.enter();
    let client = Client::connect(&endpoint).unwrap();
    let outcome = runtime.block_on(client.complete_stream(request, &mut Partial::default()));
    let failure = outcome.unwrap_err().to_string();
    let seen = heard.join().unwrap();
    assert!(!seen.is_empty(), "no request reached the server: {failure}");
    seen
}

fn plain<'a>(
    model: &str,
    messages: &'a [Message],
    effort: Option<Effort>,
) -> CompletionRequest<'a> {
    CompletionRequest::new(
        "s".to_string(),
        model.to_string(),
        messages,
        &[],
        None,
        false,
        effort,
    )
}

#[test]
fn grok_routes_every_request_of_a_session_to_its_cache() {
    let messages = [Message::system("sys"), Message::user("hi")];
    let headers = sent(
        endpoint("grok", "grok-4.6"),
        &plain("grok-4.6", &messages, None),
    );
    assert!(headers.contains("x-grok-conv-id: s\r\n"), "{headers}");
    assert!(headers.contains("\"prompt_cache_key\":\"s\""), "{headers}");
}

#[test]
fn current_claude_models_bind_thinking_and_ask_for_the_full_output() {
    let messages = tool_round();
    for (model, effort) in [
        ("claude-opus-5-5", None),
        ("claude-haiku-5-5", Some(Effort::High)),
    ] {
        let seen = sent(
            endpoint("anthropic", model),
            &plain(model, &messages, effort),
        );
        assert!(
            seen.contains("anthropic-beta: thinking-binding-controls-2026-08-01"),
            "{model}: {seen}"
        );
        assert!(
            seen.contains("\"prefix_mismatch_behavior\":\"drop_block\""),
            "{model}: {seen}"
        );
        assert!(
            seen.contains("\"display\":\"summarized\""),
            "{model}: {seen}"
        );
        assert!(seen.contains("\"max_tokens\":128000"), "{model}: {seen}");
    }
    let haiku = sent(
        endpoint("anthropic", "claude-haiku-5-5"),
        &plain("claude-haiku-5-5", &messages, Some(Effort::High)),
    );
    assert!(haiku.contains("\"effort\":\"high\""), "{haiku}");
    let older = sent(
        endpoint("anthropic", "claude-opus-4-8"),
        &plain("claude-opus-4-8", &messages, Some(Effort::High)),
    );
    assert!(
        !older.contains("anthropic-beta: thinking-binding"),
        "{older}"
    );
    assert!(!older.contains("block_binding"), "{older}");
}

#[test]
fn kimi_k3_sends_back_its_reasoning_after_a_tool_call() {
    for model in ["k3", "kimi-k3", "kimi-k2.7-code"] {
        let kimi = endpoint("kimi", model);
        let mut messages = tool_round();
        messages[2].reasoning = Some("read the file first".to_string());
        let sent = body(&kimi, &messages, None);
        assert_eq!(
            sent["messages"][2]["reasoning_content"], "read the file first",
            "{model}: {sent}"
        );
    }
}

#[test]
fn kimi_code_and_minimax_send_back_their_reasoning_and_off_means_what_each_vendor_accepts() {
    for (name, model) in [
        ("kimi", "kimi-for-coding"),
        ("kimi", "kimi-for-coding-highspeed"),
        ("minimax", "MiniMax-M3"),
    ] {
        let mut messages = tool_round();
        messages[2].reasoning = Some("read the file first".to_string());
        let sent = body(&endpoint(name, model), &messages, None);
        assert_eq!(
            sent["messages"][2]["reasoning_content"], "read the file first",
            "{model}: {sent}"
        );
    }
    let glm = body(
        &endpoint("zai", "glm-5.3"),
        &tool_round(),
        Some(Effort::None),
    );
    assert!(
        glm.get("thinking")
            .is_none_or(|thinking| thinking["type"] != "disabled"),
        "{glm}"
    );
    let minimax = body(
        &endpoint("minimax", "MiniMax-M3"),
        &tool_round(),
        Some(Effort::None),
    );
    assert!(minimax.get("thinking").is_none(), "{minimax}");
    let qwen = body(
        &endpoint("alibaba", "qwen3.7-plus"),
        &tool_round(),
        Some(Effort::None),
    );
    assert_eq!(qwen["enable_thinking"], false, "{qwen}");
}

#[test]
fn effort_reaches_cohere_and_groq_only_in_values_they_accept() {
    let messages = [Message::user("hi")];
    let cohere = body(
        &endpoint("cohere", "north-mini-code-1-0"),
        &messages,
        Some(Effort::Low),
    );
    assert_eq!(cohere["reasoning_effort"], "high", "{cohere}");
    let off = body(
        &endpoint("cohere", "north-mini-code-1-0"),
        &messages,
        Some(Effort::None),
    );
    assert_eq!(off["reasoning_effort"], "none", "{off}");
    let llama = body(
        &endpoint("groq", "llama-3.3-70b-versatile"),
        &messages,
        Some(Effort::High),
    );
    assert!(llama.get("reasoning_effort").is_none(), "{llama}");
    let oss = body(
        &endpoint("groq", "openai/gpt-oss-120b"),
        &messages,
        Some(Effort::High),
    );
    assert_eq!(oss["reasoning_effort"], "high", "{oss}");
}

#[test]
fn a_steer_after_a_tool_result_reaches_every_endpoint_after_that_result() {
    let mut messages = tool_round();
    messages.push(Message::user("also check the docs"));
    let mut checked = 0;
    for name in crate::endpoint::names().unwrap() {
        let Ok(endpoint) = crate::endpoint::resolve(Some(&name), None, crate::Need::Config) else {
            continue;
        };
        let request = CompletionRequest::new(
            "s".to_string(),
            endpoint.model.clone(),
            &messages,
            &[],
            Some(16_000),
            false,
            None,
        );
        let Ok(body) =
            Client::connect(&endpoint).and_then(|client| client.request_body(&request, false))
        else {
            continue;
        };
        let text = body.to_string();
        let result = text.find("file text");
        let steer = text.find("also check the docs");
        assert!(
            matches!((result, steer), (Some(result), Some(steer)) if result < steer),
            "{name}: {text}"
        );
        assert_eq!(
            text.matches("also check the docs").count(),
            1,
            "{name}: {text}"
        );
        checked += 1;
    }
    assert!(checked >= 20, "only {checked} endpoints built a request");
}

#[test]
fn mistral_gets_an_assistant_turn_between_a_tool_result_and_a_steer() {
    let mut messages = tool_round();
    messages.push(Message::user("also check the docs"));
    let roles = |name: &str, model: &str| -> Vec<String> {
        body(&endpoint(name, model), &messages, None)["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|message| message["role"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(
        roles("mistral", "devstral-medium-latest"),
        ["system", "user", "assistant", "tool", "assistant", "user"]
    );
    assert_eq!(
        roles("zai", "glm-5.3-flash"),
        ["system", "user", "assistant", "tool", "user"]
    );
}

#[test]
fn responses_login_uses_session_affinity() {
    let seat = crate::endpoint::login_seat("openai").unwrap();
    let client = Client::connect(&seat).unwrap();
    let messages = tool_round();
    let request = CompletionRequest::new(
        "session-7".to_string(),
        seat.model.clone(),
        &messages,
        &[],
        None,
        false,
        None,
    );
    let body = client.request_body(&request, true).unwrap();
    assert_eq!(body["prompt_cache_key"], "session-7", "{body}");
    let headers = client.headers(Some("session-7"));
    assert!(
        headers
            .iter()
            .any(|(name, value)| *name == "session-id" && value == "session-7"),
        "{headers:?}"
    );
}
