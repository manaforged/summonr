use super::*;
use crate::provider::{FunctionDefinition, Provider, ToolDefinition, ToolKind};

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
    let chat = to_chat(&request)?;
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
    let chat = to_chat(&request)?;
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
