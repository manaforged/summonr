use super::*;
use crate::provider::{FunctionDefinition, Provider, ToolDefinition, ToolKind};
use crate::provider::{Image, Message, UserInput};
use crate::runtime::Loop;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::{Duration, timeout};

#[tokio::test]
async fn native_images_and_tool_signatures_survive_the_loop_and_replay()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    for wire in [Wire::Anthropic, Wire::Gemini] {
        for streaming in [false, true] {
            conversation(wire, streaming).await?;
        }
    }
    Ok(())
}

async fn conversation(
    wire: Wire,
    streaming: bool,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let workspace = tempfile::tempdir()?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let model = if wire == Wire::Gemini {
        "gemini-3-pro-preview"
    } else {
        "claude-sonnet-4-5"
    };
    let endpoint = crate::endpoint::Endpoint {
        name: "fixture".into(),
        model: model.into(),
        base_url: Some(format!("http://{address}/v1")),
        api_key: Some("fixture".into()),
        source: None,
        oauth: None,
        body: None,
        context_window: None,
        idle_timeout_secs: Some(2),
        effort: None,
        price: None,
        env: Default::default(),
        headers: Default::default(),
        wire,
    };
    let mut client = Client::connect(&endpoint)?;
    if streaming {
        client = client.on_delta(|_| {});
    }
    let server = async {
        let mut requests = Vec::new();
        for first in [true, false, false] {
            let (mut stream, _) = listener.accept().await?;
            let mut bytes = Vec::new();
            let mut byte = [0];
            while !bytes.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).await?;
                bytes.push(byte[0]);
            }
            let length = String::from_utf8_lossy(&bytes)
                .lines()
                .find_map(|line| {
                    let (key, value) = line.split_once(':')?;
                    key.eq_ignore_ascii_case("content-length").then(|| {
                        value
                            .trim()
                            .parse::<usize>()
                            .expect("request content length")
                    })
                })
                .expect("request body length");
            let mut body = vec![0; length];
            stream.read_exact(&mut body).await?;
            requests.push(serde_json::from_slice::<Value>(&body)?);
            let body = reply(wire, streaming, first);
            let content_type = if streaming {
                "text/event-stream"
            } else {
                "application/json"
            };
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await?;
        }
        Ok::<_, Box<dyn std::error::Error>>(requests)
    };
    let turn = async {
        let mut agent = Loop::with_session(client.clone(), model, workspace.path(), "system")?;
        agent.set_effort(Some(crate::provider::Effort::High));
        let input = UserInput {
            text: "inspect image".into(),
            source: None,
            images: vec![Image::encode(
                "image/png",
                &STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a1S0AAAAASUVORK5CYII=")?,
            )],
        };
        let result = agent.turn_with(input, |_| {}).await?;
        assert!(result.text.contains("done"));
        let mut state = crate::durable::log::State::default();
        for line in crate::durable::log::read(agent.session_path())? {
            state.apply(line);
        }
        let replay = state.replay();
        drop(agent);
        let mut resumed =
            Loop::with_session(client, model, workspace.path(), "system")?.restore_replay(replay);
        assert!(resumed.turn("continue").await?.text.contains("done"));
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let (requests, ()) = timeout(Duration::from_secs(15), async {
        tokio::try_join!(server, turn)
    })
    .await??;
    for request in &requests {
        let image = if wire == Wire::Gemini {
            &request["contents"][0]["parts"][1]["inline_data"]["data"]
        } else {
            &request["messages"][0]["content"][1]["source"]["data"]
        };
        assert_eq!(
            image,
            "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+a1S0AAAAASUVORK5CYII=",
            "{wire:?} streaming={streaming}: {request}"
        );
    }
    if wire == Wire::Gemini {
        for request in &requests[1..] {
            let mut parts = request["contents"]
                .as_array()
                .expect("contents")
                .iter()
                .flat_map(|message| message["parts"].as_array().expect("parts"));
            let call = parts
                .find(|part| part.get("functionCall").is_some())
                .expect("tool call");
            assert_eq!(
                call["thoughtSignature"], "fixture-signature",
                "streaming={streaming}: {request}"
            );
            assert!(
                !request
                    .to_string()
                    .contains("skip_thought_signature_validator")
            );
        }
    } else {
        for request in &requests[1..] {
            let assistant = request["messages"]
                .as_array()
                .expect("messages")
                .iter()
                .find(|message| message["role"] == "assistant")
                .expect("assistant continuation");
            assert_eq!(
                assistant["content"],
                anthropic_content(true),
                "streaming={streaming}: {request}"
            );
        }
    }
    Ok(())
}

fn reply(wire: Wire, streaming: bool, first: bool) -> String {
    if wire == Wire::Gemini {
        let parts = if first {
            json!([{"functionCall":{"name":"list_dir","args":{"path":"."}},"thoughtSignature":"fixture-signature"}])
        } else {
            json!([{"text":"done"}])
        };
        let body = json!({"candidates":[{"content":{"role":"model","parts":parts},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":2,"totalTokenCount":12}});
        return if streaming {
            event("message", body)
        } else {
            body.to_string()
        };
    }
    let content = anthropic_content(first);
    let stop = if first { "tool_use" } else { "end_turn" };
    if streaming {
        let mut body = message_start();
        for (index, block) in content
            .as_array()
            .expect("fixture content")
            .iter()
            .enumerate()
        {
            let (start, deltas) = match block["type"].as_str().expect("block type") {
                "thinking" => (
                    json!({"type":"thinking","thinking":"","signature":"signed-"}),
                    vec![
                        json!({"type":"thinking_delta","thinking":block["thinking"]}),
                        json!({"type":"signature_delta","signature":"fragment-"}),
                        json!({"type":"signature_delta","signature":"fragment"}),
                    ],
                ),
                "tool_use" => (
                    json!({"type":"tool_use","id":"fixture-call","name":"list_dir","input":{}}),
                    vec![json!({"type":"input_json_delta","partial_json":"{\"path\":\".\"}"})],
                ),
                "text" => (
                    json!({"type":"text","text":""}),
                    vec![json!({"type":"text_delta","text":block["text"]})],
                ),
                _ => (block.clone(), Vec::new()),
            };
            body += &event(
                "content_block_start",
                json!({"type":"content_block_start","index":index,"content_block":start}),
            );
            for delta in deltas {
                body += &event(
                    "content_block_delta",
                    json!({"type":"content_block_delta","index":index,"delta":delta}),
                );
            }
            body += &event(
                "content_block_stop",
                json!({"type":"content_block_stop","index":index}),
            );
        }
        body + &terminal(stop)
    } else {
        json!({"id":"fixture","type":"message","role":"assistant","model":"claude-sonnet-4-5","content":content,"stop_reason":stop,"usage":{"input_tokens":10,"output_tokens":2}}).to_string()
    }
}

fn anthropic_content(first: bool) -> Value {
    if first {
        json!([
            {"type":"thinking","thinking":"Inspect the workspace.","signature":"signed-fragment-fragment"},
            {"type":"redacted_thinking","data":"opaque-redacted-fixture"},
            {"type":"text","text":"I will inspect the files."},
            {"type":"tool_use","id":"fixture-call","name":"list_dir","input":{"path":"."}}
        ])
    } else {
        json!([{"type":"text","text":"done"}])
    }
}

#[test]
fn native_continuation_is_scoped_to_its_model_and_wire() -> Result<()> {
    let message = Message {
        role: Role::Assistant,
        content: String::new(),
        images: Vec::new(),
        tool_call_id: None,
        reasoning: None,
        tool_calls: vec![ToolCall {
            id: "call".into(),
            kind: ToolKind::Function,
            function: ToolFunction {
                name: "list_dir".into(),
                arguments: "{}".into(),
            },
            continuation: Some(Continuation::Gemini {
                model: "gemini-3-pro-preview".into(),
                signatures: vec!["fixture-signature".into()],
            }),
        }],
    };
    let mut request = CompletionRequest {
        model: "gemini-3-pro-preview".into(),
        messages: vec![message.clone()],
        tools: Vec::new(),
        max_output_tokens: None,
        emit_deltas: false,
        effort: None,
    };
    assert!(
        to_chat(&request, Wire::Gemini)?.messages[0]
            .content
            .contains_thought_signature()
    );
    assert!(
        !to_chat(&request, Wire::Anthropic)?.messages[0]
            .content
            .contains_thought_signature()
    );
    request.model = "gemini-2.5-pro".into();
    assert!(
        !to_chat(&request, Wire::Gemini)?.messages[0]
            .content
            .contains_thought_signature()
    );
    let completions = crate::provider::message_value(&message)?;
    assert!(completions["tool_calls"][0].get("continuation").is_none());
    assert!(!completions.to_string().contains("fixture-signature"));
    request.messages[0].tool_calls[0].continuation = Some(Continuation::Anthropic {
        model: "claude-sonnet-4-5".into(),
        blocks: anthropic_content(true)
            .as_array()
            .expect("fixture content")
            .clone(),
    });
    request.model = "claude-sonnet-4-5".into();
    let native = serde_json::to_string(&to_chat(&request, Wire::Anthropic)?)?;
    assert!(native.contains("opaque-redacted-fixture"));
    assert!(
        !serde_json::to_string(&to_chat(&request, Wire::Gemini)?)?
            .contains("opaque-redacted-fixture")
    );
    request.model = "claude-sonnet-4-6".into();
    assert!(
        !serde_json::to_string(&to_chat(&request, Wire::Anthropic)?)?
            .contains("opaque-redacted-fixture")
    );
    assert!(
        !crate::provider::message_value(&request.messages[0])?
            .to_string()
            .contains("opaque-redacted-fixture")
    );
    request.messages[0].tool_calls[0].function.arguments = "{".into();
    let error = to_chat(&request, Wire::Gemini)
        .expect_err("invalid arguments must not become an empty object");
    assert_eq!(
        error.provider_failure().map(|failure| failure.kind),
        Some(ProviderFailureKind::MalformedToolCall)
    );
    Ok(())
}

async fn anthropic_stream(body: &str) -> Result<AssistantMessage> {
    native_stream(Wire::Anthropic, "claude-sonnet-4-5", body).await
}

async fn native_stream(wire: Wire, model: &str, body: &str) -> Result<AssistantMessage> {
    native_response(wire, model, body, 200, true).await
}

async fn native_response(
    wire: Wire,
    model: &str,
    body: &str,
    status: u16,
    streaming: bool,
) -> Result<AssistantMessage> {
    use std::collections::BTreeMap;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let body = body.to_string();
    let server = thread::spawn(move || -> std::io::Result<()> {
        let (mut stream, _) = listener.accept()?;
        let mut request = [0_u8; 16 * 1024];
        let _read = stream.read(&mut request)?;
        write!(
            stream,
            "HTTP/1.1 {status} Response\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            if streaming {
                "text/event-stream"
            } else {
                "application/json"
            },
            body.len(),
            body
        )?;
        Ok(())
    });
    let endpoint = crate::endpoint::Endpoint {
        name: "native-stream-test".into(),
        model: model.into(),
        base_url: Some(format!("http://{address}/v1")),
        api_key: Some("test-key".into()),
        source: None,
        oauth: None,
        body: None,
        context_window: None,
        idle_timeout_secs: Some(2),
        effort: None,
        price: None,
        env: BTreeMap::new(),
        headers: BTreeMap::new(),
        wire,
    };
    let client = Client::connect(&endpoint)?.on_delta(|_| {});
    let request = CompletionRequest {
        model: endpoint.model,
        messages: vec![crate::provider::Message::user("hello")],
        tools: Vec::new(),
        max_output_tokens: Some(128),
        emit_deltas: streaming,
        effort: None,
    };
    let result = client.complete(&request).await;
    server
        .join()
        .map_err(|_error| Error::Config("native stream fixture panicked".into()))??;
    result
}

#[tokio::test]
async fn native_http_failures_keep_status_and_retry_category() -> Result<()> {
    for streaming in [false, true] {
        for (status, kind) in [
            (401, ProviderFailureKind::Authentication),
            (429, ProviderFailureKind::RateLimited),
            (503, ProviderFailureKind::Server),
        ] {
            let error = native_response(
                Wire::Anthropic,
                "claude-sonnet-4-5",
                r#"{"error":{"message":"request refused"}}"#,
                status,
                streaming,
            )
            .await
            .expect_err("provider error response");
            let failure = error.provider_failure().expect("typed provider failure");
            assert_eq!(failure.status, Some(status), "{error}");
            assert_eq!(failure.kind, kind, "{error}");
            assert!(!failure.partial_output);
        }
    }
    Ok(())
}

fn event(name: &str, data: serde_json::Value) -> String {
    format!("event: {name}\ndata: {data}\n\n")
}

fn message_start() -> String {
    event(
        "message_start",
        serde_json::json!({
            "type": "message_start",
            "message": {
                "id": "fixture", "type": "message", "role": "assistant",
                "model": "claude-sonnet-4-5", "content": [],
                "usage": {"input_tokens": 1, "output_tokens": 0}
            }
        }),
    )
}

fn terminal(reason: &str) -> String {
    event(
        "message_delta",
        serde_json::json!({
            "type": "message_delta", "delta": {"stop_reason": reason},
            "usage": {"output_tokens": 2}
        }),
    ) + &event("message_stop", serde_json::json!({"type": "message_stop"}))
}

#[tokio::test]
async fn native_stream_uses_one_completed_tool_call() -> Result<()> {
    let arguments = serde_json::json!({"path": "result.txt", "content": "done\n"});
    let body = message_start()
        + &event(
            "content_block_start",
            serde_json::json!({
                "type": "content_block_start", "index": 0,
                "content_block": {"type": "tool_use", "id": "write", "name": "write_file", "input": {}}
            }),
        )
        + &event(
            "content_block_delta",
            serde_json::json!({
                "type": "content_block_delta", "index": 0,
                "delta": {"type": "input_json_delta", "partial_json": arguments.to_string()}
            }),
        )
        + &event(
            "content_block_stop",
            serde_json::json!({"type": "content_block_stop", "index": 0}),
        )
        + &terminal("tool_use");
    let message = anthropic_stream(&body).await?;
    assert_eq!(message.stop_reason, StopReason::ToolCalls);
    assert_eq!(message.tool_calls.len(), 1);
    assert_eq!(message.tool_calls[0].id, "write");
    assert_eq!(message.tool_calls[0].function.name, "write_file");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&message.tool_calls[0].function.arguments)?,
        arguments
    );
    Ok(())
}

#[tokio::test]
async fn native_stream_rejects_eof_with_partial_output() -> Result<()> {
    let body = message_start()
        + &event(
            "content_block_start",
            serde_json::json!({
                "type": "content_block_start", "index": 0,
                "content_block": {"type": "text", "text": ""}
            }),
        )
        + &event(
            "content_block_delta",
            serde_json::json!({
                "type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": "partial"}
            }),
        );
    let error = anthropic_stream(&body)
        .await
        .expect_err("EOF before message_stop must fail");
    let failure = error.provider_failure().expect("provider failure");
    assert_eq!(failure.kind, ProviderFailureKind::Malformed);
    assert!(failure.partial_output);
    Ok(())
}

#[tokio::test]
async fn native_stream_preserves_partial_output_on_parse_error() -> Result<()> {
    let body = message_start()
        + &event(
            "content_block_start",
            serde_json::json!({
                "type": "content_block_start", "index": 0,
                "content_block": {"type": "text", "text": ""}
            }),
        )
        + &event(
            "content_block_delta",
            serde_json::json!({
                "type": "content_block_delta", "index": 0,
                "delta": {"type": "text_delta", "text": "partial"}
            }),
        )
        + "event: content_block_delta\ndata: {\n\n";
    let error = anthropic_stream(&body)
        .await
        .expect_err("malformed event after output must fail");
    let failure = error.provider_failure().expect("provider failure");
    assert_eq!(failure.kind, ProviderFailureKind::Malformed);
    assert!(failure.partial_output);
    Ok(())
}

#[tokio::test]
async fn native_anthropic_rejects_incomplete_or_invalid_continuation() -> Result<()> {
    let start = message_start()
        + &event(
            "content_block_start",
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        )
        + &event(
            "content_block_delta",
            json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial"}}),
        );
    for tail in [
        terminal("end_turn"),
        event(
            "content_block_delta",
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"wrong block"}}),
        ) + &terminal("end_turn"),
        event(
            "content_block_delta",
            json!({"type":"content_block_delta","index":0,"delta":{"type":"unknown_delta","data":"unsupported"}}),
        ) + &terminal("end_turn"),
    ] {
        let error = anthropic_stream(&(start.clone() + &tail))
            .await
            .expect_err("invalid native continuation must fail");
        let failure = error.provider_failure().expect("provider failure");
        assert_eq!(failure.kind, ProviderFailureKind::Malformed);
        assert!(
            failure.partial_output,
            "failure must retain emitted output: {error}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn native_stream_preserves_length_stop_reason() -> Result<()> {
    let body = message_start() + &terminal("max_tokens");
    let message = anthropic_stream(&body).await?;
    assert_eq!(message.stop_reason, StopReason::Length);
    Ok(())
}

#[test]
fn native_stop_reasons_keep_completion_semantics() {
    use genai::chat::StopReason as GenaiStop;

    assert_eq!(
        from_genai_stop(&GenaiStop::Completed("stop".into()), false).expect("completion"),
        StopReason::Stop
    );
    assert_eq!(
        from_genai_stop(&GenaiStop::StopSequence("stop_sequence".into()), false)
            .expect("stop sequence"),
        StopReason::Stop
    );
    assert_eq!(
        from_genai_stop(&GenaiStop::ToolCall("tool_use".into()), false).expect("tool call"),
        StopReason::ToolCalls
    );
    assert_eq!(
        from_genai_stop(&GenaiStop::MaxTokens("max_tokens".into()), false).expect("token limit"),
        StopReason::Length
    );
    let unsupported = from_genai_stop(&GenaiStop::Other("cancelled".into()), true)
        .expect_err("unknown stop reason is not a successful completion");
    assert_eq!(
        unsupported
            .provider_failure()
            .map(|failure| (failure.kind, failure.partial_output)),
        Some((ProviderFailureKind::Malformed, true))
    );
}

#[test]
fn native_content_filter_is_terminal_without_output() {
    use genai::chat::StopReason as GenaiStop;

    let filtered = from_genai_stop(&GenaiStop::ContentFilter("SAFETY".into()), false)
        .expect_err("content filter is not a successful completion");
    assert_eq!(
        filtered
            .provider_failure()
            .map(|failure| (failure.kind, failure.partial_output)),
        Some((ProviderFailureKind::Http, false))
    );
}

#[tokio::test]
async fn gemini_stream_rejects_eof_without_finish_reason() -> Result<()> {
    let body = event(
        "message",
        serde_json::json!({
            "candidates": [{"content": {"role": "model", "parts": [{"text": "partial"}]}}],
            "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1, "totalTokenCount": 2}
        }),
    );
    let error = native_stream(Wire::Gemini, "gemini-2.5-pro", &body)
        .await
        .expect_err("Gemini EOF without finishReason must fail");
    let failure = error.provider_failure().expect("provider failure");
    assert_eq!(failure.kind, ProviderFailureKind::Malformed);
    assert!(failure.partial_output);
    Ok(())
}

#[tokio::test]
async fn gemini_stream_accepts_finish_reason() -> Result<()> {
    let body = event(
        "message",
        serde_json::json!({
            "candidates": [{
                "content": {"role": "model", "parts": [{"text": "done"}]},
                "finishReason": "STOP"
            }],
            "usageMetadata": {"promptTokenCount": 1, "candidatesTokenCount": 1, "totalTokenCount": 2}
        }),
    );
    let message = native_stream(Wire::Gemini, "gemini-2.5-pro", &body).await?;
    assert_eq!(message.content.as_deref(), Some("done"));
    assert_eq!(message.stop_reason, StopReason::Stop);
    Ok(())
}

#[tokio::test]
async fn native_anthropic_request_honors_idle_timeout() -> Result<()> {
    use std::collections::BTreeMap;
    use std::io::Read;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let (reached_tx, reached_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let server = thread::spawn(move || -> std::io::Result<()> {
        let (mut stream, _) = listener.accept()?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        let mut request = [0_u8; 1];
        stream.read_exact(&mut request)?;
        reached_tx
            .send(())
            .map_err(|_error| std::io::Error::other("request receiver dropped"))?;
        release_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_error| std::io::Error::other("release receiver timed out"))?;
        Ok(())
    });

    let endpoint = crate::endpoint::Endpoint {
        name: "anthropic-test".into(),
        model: "claude-sonnet-4-5".into(),
        base_url: Some(format!("http://{address}/v1")),
        api_key: Some("test-key".into()),
        source: None,
        oauth: None,
        body: None,
        context_window: None,
        idle_timeout_secs: Some(1),
        effort: None,
        price: None,
        env: BTreeMap::new(),
        headers: BTreeMap::new(),
        wire: Wire::Anthropic,
    };
    let client = Client::connect(&endpoint)?;
    let request = CompletionRequest {
        model: endpoint.model.clone(),
        messages: vec![crate::provider::Message::user("hello")],
        tools: Vec::new(),
        max_output_tokens: Some(16),
        emit_deltas: false,
        effort: None,
    };

    let started = Instant::now();
    let outcome = tokio::time::timeout(Duration::from_secs(4), client.complete(&request)).await;
    let elapsed = started.elapsed();
    assert!(
        reached_rx.recv_timeout(Duration::from_secs(1)).is_ok(),
        "native Anthropic request did not reach the local server"
    );
    release_tx
        .send(())
        .expect("server still holds the connection");
    server
        .join()
        .map_err(|_error| Error::Config("native timeout fixture panicked".into()))??;
    let error = outcome
        .expect("native request exceeded the bounded outer timeout")
        .expect_err("stalled native request should fail on its idle timeout");
    assert!(
        elapsed < Duration::from_secs(3),
        "native request did not fail within the idle timeout window: {elapsed:?}"
    );
    assert_eq!(
        error.provider_failure().map(|failure| failure.kind),
        Some(ProviderFailureKind::Transport),
        "expected a native transport error, got: {error}"
    );
    Ok(())
}

#[test]
fn chat_options_leave_the_output_limit_unset() {
    let options = chat_options(&[], None, None);
    assert_eq!(options.max_tokens, None);
}

#[test]
fn native_kind_binds_anthropic_and_gemini_only() {
    assert_eq!(
        Client::native_kind(Wire::Anthropic),
        Some(AdapterKind::Anthropic)
    );
    assert_eq!(Client::native_kind(Wire::Gemini), Some(AdapterKind::Gemini));
    assert_eq!(Client::native_kind(Wire::Completions), None);
}

#[test]
fn to_chat_lifts_system_and_tools() -> Result<()> {
    let request = CompletionRequest {
        model: "claude-sonnet-4-5".into(),
        messages: vec![
            crate::provider::Message::system("be brief"),
            crate::provider::Message::user("hello"),
        ],
        tools: vec![ToolDefinition {
            kind: ToolKind::Function,
            function: FunctionDefinition {
                name: "read_file".into(),
                description: "Read a file".into(),
                parameters: serde_json::json!({"type": "object"}),
            },
        }],
        max_output_tokens: Some(128),
        emit_deltas: true,
        effort: None,
    };
    let chat = to_chat(&request, Wire::Anthropic)?;
    assert_eq!(chat.system.as_deref(), Some("be brief"));
    assert_eq!(chat.messages.len(), 1);
    let tools = chat.tools.expect("tools");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name.as_str(), "read_file");
    Ok(())
}

#[test]
fn to_chat_marks_the_last_message_for_prompt_caching() -> Result<()> {
    let request = CompletionRequest {
        model: "claude-sonnet-4-5".into(),
        messages: vec![
            crate::provider::Message::system("be brief"),
            crate::provider::Message::user("hello"),
        ],
        tools: Vec::new(),
        max_output_tokens: Some(128),
        emit_deltas: false,
        effort: None,
    };
    let chat = to_chat(&request, Wire::Anthropic)?;
    let last = chat.messages.last().expect("a message");
    assert!(matches!(
        last.options.as_ref().and_then(|o| o.cache_control.as_ref()),
        Some(CacheControl::Ephemeral)
    ));
    Ok(())
}

#[test]
fn chat_options_carry_the_effort_dial() {
    use crate::provider::Effort;
    use genai::chat::ReasoningEffort;
    assert!(chat_options(&[], None, None).reasoning_effort.is_none());
    assert_eq!(
        chat_options(&[], Some(8192), Some(Effort::High)).max_tokens,
        Some(8192)
    );
    assert_eq!(chat_options(&[], None, None).max_tokens, None);
    assert!(
        chat_options(&[], None, Some(Effort::None))
            .reasoning_effort
            .is_none(),
        "none leaves the wire field out on every wire"
    );
    assert!(matches!(
        chat_options(&[], None, Some(Effort::Low)).reasoning_effort,
        Some(ReasoningEffort::Low)
    ));
}
