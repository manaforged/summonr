use super::*;

fn request(messages: Vec<Message>) -> CompletionRequest {
    CompletionRequest {
        model: "m".to_string(),
        messages,
        tools: Vec::new(),
        max_output_tokens: None,
        emit_deltas: true,
        effort: None,
    }
}

#[test]
fn durable_request_captures_input_and_images_but_not_ui_delivery() -> Result<()> {
    let first = request(vec![Message::system("same"), Message::user("first")]);
    let second = request(vec![Message::system("same"), Message::user("second")]);
    assert_ne!(first.durable_bytes()?, second.durable_bytes()?);

    let mut with_image = first.clone();
    with_image.messages[1].images = vec![Image::encode("image/png", b"image")];
    assert_ne!(first.durable_bytes()?, with_image.durable_bytes()?);

    let mut quiet = first.clone();
    quiet.emit_deltas = false;
    assert_eq!(first.durable_bytes()?, quiet.durable_bytes()?);
    Ok(())
}

#[test]
fn model_list_sorts_deduplicates_and_ignores_bad_rows() -> Result<()> {
    let models = model_ids(&json!({
        "data": [
            {"id": "qwen-3"},
            {"object": "model"},
            {"id": "glm-5.3"},
            {"id": "qwen-3"}
        ]
    }))?;
    assert_eq!(models, ["glm-5.3", "qwen-3"]);
    model_ids(&json!({"data": []})).unwrap_err();
    Ok(())
}

#[test]
fn context_metadata_selects_the_active_model_and_valid_limits() {
    let models = json!({
        "data": [
            {"id": "other", "context_length": 8192},
            {"id": "qwen", "context_length": 262144}
        ]
    });
    assert_eq!(listed(&models, "qwen"), Some(262_144));
    assert_eq!(listed(&models, "missing"), None);
    assert_eq!(
        listed(
            &json!({"data": [{"id": "other", "context_length": 8192}]}),
            "missing"
        ),
        None
    );
    assert_eq!(
        loaded(&json!({
            "default_generation_settings": {"n_ctx": 65536}
        })),
        Some(65_536)
    );
    assert_eq!(loaded(&json!({"n_ctx": 0})), None);
}

#[tokio::test]
async fn context_reads_native_model_limits_with_provider_authentication() -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    for (wire, version, model, field, header) in [
        (
            Wire::Anthropic,
            "v1",
            "claude-fixture",
            "max_input_tokens",
            "x-api-key: fixture-key",
        ),
        (
            Wire::Gemini,
            "v1beta",
            "models/gemini-fixture",
            "inputTokenLimit",
            "x-goog-api-key: fixture-key",
        ),
    ] {
        for (value, expected) in [
            (json!(131072), Some(131072)),
            (json!(0), None),
            (Value::Null, None),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await?;
            let address = listener.local_addr()?;
            let client = Client::build(
                &format!("http://{address}/{version}"),
                Auth::bearer("fixture-key"),
                None,
                vec![("x-fixture".to_string(), "metadata".to_string())],
                wire,
                None,
                None,
            )?;
            let server = async {
                let (mut connection, _) = listener.accept().await.expect("metadata request");
                let mut bytes = Vec::new();
                while !bytes.windows(4).any(|chunk| chunk == b"\r\n\r\n") {
                    let mut chunk = [0; 1024];
                    let count = connection.read(&mut chunk).await.expect("headers");
                    assert!(count > 0);
                    bytes.extend_from_slice(&chunk[..count]);
                }
                let request = String::from_utf8_lossy(&bytes);
                let name = model.strip_prefix("models/").unwrap_or(model);
                assert!(request.starts_with(&format!("GET /{version}/models/{name} ")));
                let headers = request.to_ascii_lowercase();
                assert!(headers.contains(header));
                assert!(headers.contains("x-fixture: metadata"));
                if wire == Wire::Anthropic {
                    assert!(headers.contains("anthropic-version: 2023-06-01"));
                }
                let body = json!({field: value}).to_string();
                connection.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.expect("metadata response");
            };
            let ((), limit) = tokio::time::timeout(Duration::from_secs(6), async {
                tokio::join!(server, client.context(model))
            })
            .await
            .expect("bounded metadata probe");
            assert_eq!(limit, expected);
        }
    }
    Ok(())
}

#[tokio::test]
async fn context_reads_the_loaded_llama_server_window() -> Result<()> {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let server = std::thread::spawn(move || -> std::io::Result<()> {
        let (mut stream, _) = listener.accept()?;
        let mut request = [0_u8; 2048];
        let count = stream.read(&mut request)?;
        let request = String::from_utf8_lossy(&request[..count]);
        assert!(request.starts_with("GET /props?model=qwen "));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer key")
        );
        let body = r#"{"default_generation_settings":{"n_ctx":131072}}"#;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    });
    let client = Client::new("key", format!("http://{address}/v1"))?;

    assert_eq!(client.context("qwen").await, Some(131_072));

    server
        .join()
        .map_err(|_panic| Error::Config("context fixture panicked".to_string()))??;
    Ok(())
}

#[test]
fn usage_reads_cached_tokens_from_details_or_flat() {
    let nested: Usage = serde_json::from_str(
        r#"{"prompt_tokens":1224,"completion_tokens":2,"prompt_tokens_details":{"cached_tokens":708}}"#,
    )
    .unwrap_or_default();
    assert_eq!(nested.cached_tokens, 708);
    let flat: Usage = serde_json::from_str(r#"{"prompt_tokens":5,"prompt_cache_hit_tokens":4}"#)
        .unwrap_or_default();
    assert_eq!(flat.cached_tokens, 4);
    let none: Usage = serde_json::from_str(r#"{"prompt_tokens":5}"#).unwrap_or_default();
    assert_eq!(none.cached_tokens, 0);
    let round: Usage = serde_json::from_str(&serde_json::to_string(&nested).unwrap_or_default())
        .unwrap_or_default();
    assert_eq!(round, nested, "serialized form reads back");
}

#[test]
fn usage_reads_reasoning_tokens_from_details_or_flat() {
    let nested: Usage = serde_json::from_str(
        r#"{"completion_tokens":42,"completion_tokens_details":{"reasoning_tokens":31}}"#,
    )
    .unwrap_or_default();
    assert_eq!(nested.reasoning_tokens, 31);
    let flat: Usage = serde_json::from_str(r#"{"reasoning_tokens":7}"#).unwrap_or_default();
    assert_eq!(flat.reasoning_tokens, 7);
}

#[test]
fn frontier_answer_omits_reasoning_content() -> Result<()> {
    let client = Client::new("k", "http://h/v1")?;
    let mut message = Message::user("hi");
    message.role = Role::Assistant;
    message.content = "ok".to_string();
    let body = client.request_body(&request(vec![message]), false)?;
    assert_eq!(body["messages"][0]["content"], "ok");
    assert!(body["messages"][0].get("reasoning_content").is_none());
    Ok(())
}

#[test]
fn think_tags_hoist_out_of_the_answer() -> Result<()> {
    let reply: Completion =
        serde_json::from_str(r#"{"choices":[{"message":{"content":"<think>secret</think>ok"}}]}"#)
            .unwrap_or_else(|error| panic!("{error}"));
    let message = finish(
        reply
            .choices
            .into_iter()
            .next()
            .map(|choice| choice.message)
            .unwrap_or_default(),
    )?;
    assert_eq!(message.content.as_deref(), Some("ok"));
    assert_eq!(message.reasoning_content.as_deref(), Some("secret"));
    Ok(())
}

#[test]
fn object_reasoning_becomes_reasoning_content() {
    let reply: Completion = serde_json::from_str(
        r#"{"choices":[{"message":{"content":"ok","reasoning":{"content":"plan"}}}]}"#,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    let message = reply
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message)
        .unwrap_or_default();
    assert_eq!(message.content.as_deref(), Some("ok"));
    assert_eq!(message.reasoning_content.as_deref(), Some("plan"));
}

#[test]
fn reasoning_rides_the_wire_as_reasoning_content() -> Result<()> {
    let client = Client::new("k", "http://h/v1")?;
    let mut message = Message::user("hi");
    message.role = Role::Assistant;
    message.reasoning = Some("because".to_string());
    let body = client.request_body(&request(vec![message]), false)?;
    assert_eq!(body["messages"][0]["reasoning_content"], "because");
    assert_eq!(body["messages"][0]["role"], "assistant");
    assert!(body["messages"][0].get("tool_calls").is_none());
    Ok(())
}

#[test]
fn images_use_openai_content_parts_without_changing_text_messages() -> Result<()> {
    let client = Client::new("k", "http://h/v1")?;
    let text = client.request_body(&request(vec![Message::user("hi")]), false)?;
    assert_eq!(text["messages"][0]["content"], "hi");

    let input = UserInput {
        text: "describe this".to_string(),
        source: None,
        images: vec![Image::encode("image/png", b"\x89PNG\r\n\x1a\n")],
    };
    let body = client.request_body(&request(vec![Message::user(input)]), false)?;
    let content = &body["messages"][0]["content"];
    assert_eq!(
        content[0],
        json!({ "type": "text", "text": "describe this" })
    );
    assert_eq!(content[1]["type"], "image_url");
    assert_eq!(
        content[1]["image_url"]["url"],
        "data:image/png;base64,iVBORw0KGgo="
    );
    assert!(body["messages"][0].get("images").is_none());
    Ok(())
}

#[test]
fn body_table_overlays_the_request() -> Result<()> {
    let mut client = Client::new("k", "http://h/v1/")?;
    client.body =
        Some(json!({"temperature": 0.6, "chat_template_kwargs": {"reasoning_effort": "low"}}));
    let body = client.request_body(&request(vec![Message::user("hi")]), true)?;
    assert_eq!(client.url, "http://h/v1/chat/completions");
    assert_eq!(body["temperature"], 0.6);
    assert_eq!(body["chat_template_kwargs"]["reasoning_effort"], "low");
    assert!(body.get("max_tokens").is_none());
    assert_eq!(body["stream_options"]["include_usage"], true);
    Ok(())
}

#[test]
fn endpoint_can_set_a_round_output_limit() -> Result<()> {
    let mut client = Client::new("k", "http://h/v1")?;
    client.body = Some(json!({"max_tokens": 4096}));
    let body = client.request_body(&request(vec![Message::user("hi")]), false)?;
    assert_eq!(body["max_tokens"], 4096);
    Ok(())
}

#[test]
fn body_extras_reject_provider_owned_fields() {
    for key in ["model", "messages", "stream", "tools", "stream_options"] {
        let body = Some(json!({ key: "override" }));
        let error = Client::build(
            "http://h/v1",
            Auth::bearer("k"),
            body,
            Vec::new(),
            crate::endpoint::Wire::Completions,
            None,
            None,
        );
        assert!(error.is_err(), "reserved key {key} was accepted");
    }
}

#[test]
fn model_request_timeout_is_opt_in() -> Result<()> {
    assert_eq!(request_timeout(None)?, None);
    assert_eq!(request_timeout(Some(600))?, Some(Duration::from_secs(600)));
    request_timeout(Some(0)).unwrap_err();
    Ok(())
}

#[test]
fn http_failures_keep_status_and_category() {
    let cases = [
        (401, "no", ProviderFailureKind::Authentication),
        (429, "slow", ProviderFailureKind::RateLimited),
        (503, "down", ProviderFailureKind::Server),
        (
            500,
            "Failed to parse tool call arguments as JSON",
            ProviderFailureKind::MalformedToolCall,
        ),
        (
            400,
            "context length exceeded",
            ProviderFailureKind::ContextOverflow,
        ),
        (
            400,
            "request 500 says connection",
            ProviderFailureKind::Http,
        ),
    ];
    for (status, body, kind) in cases {
        let error = response_failure(status, body, None);
        let failure = error
            .provider_failure()
            .unwrap_or_else(|| panic!("{error}"));
        assert_eq!(failure.status, Some(status));
        assert_eq!(failure.kind, kind);
        assert_eq!(failure.retry_after, None);
    }
    let limited = response_failure(429, "slow down", Some(17));
    assert_eq!(
        limited.provider_failure().and_then(|f| f.retry_after),
        Some(17)
    );
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::RETRY_AFTER, "30".parse().expect("header"));
    assert_eq!(retry_after(&headers), Some(30));
    headers.insert(
        reqwest::header::RETRY_AFTER,
        "Wed, 21 Oct 2026 07:28:00 GMT".parse().expect("header"),
    );
    assert_eq!(
        retry_after(&headers),
        None,
        "HTTP dates fall back to backoff"
    );
}

#[test]
fn tool_call_fragments_fold_by_index() {
    let mut calls = Vec::new();
    let part = |index, id: Option<&str>, name: Option<&str>, args: Option<&str>| CallDelta {
        index: Some(index),
        id: id.map(str::to_string),
        function: Some(FunctionDelta {
            name: name.map(str::to_string),
            arguments: args.map(str::to_string),
        }),
    };
    merge_call(
        &mut calls,
        part(0, Some("c1"), Some("read_file"), Some("{\"pa")),
    );
    merge_call(&mut calls, part(0, None, None, Some("th\":1}")));
    merge_call(&mut calls, part(1, Some("c2"), Some("bash"), Some("{}")));
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].id, "c1");
    assert_eq!(calls[0].function.arguments, "{\"path\":1}");
    assert_eq!(calls[1].function.name, "bash");
}

#[test]
fn reasoning_comes_back_in_the_message() {
    let reply: Completion = serde_json::from_str(
        r#"{"choices":[{"message":{"content":"ok","reasoning_content":"think"}}],"usage":{"prompt_tokens":3,"completion_tokens":1,"prompt_tokens_details":{"cached_tokens":2}}}"#,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let message = reply
        .choices
        .into_iter()
        .next()
        .map(|c| c.message)
        .unwrap_or_default();
    assert_eq!(message.into_message().reasoning.as_deref(), Some("think"));
}

#[test]
fn nonstream_finish_reason_is_deserialized() {
    let reply: Completion = serde_json::from_str(
        r#"{"choices":[{"message":{"reasoning_content":"think"},"finish_reason":"length"}]}"#,
    )
    .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(reply.choices[0].finish_reason, Some(StopReason::Length));
}

#[test]
fn only_openai_host_receives_a_prompt_cache_key() -> Result<()> {
    let req = request(vec![Message::user("hi")]);
    let generic = Client::new("k", "https://api.deepseek.com/v1")?.request_body(&req, false)?;
    assert!(generic.get("prompt_cache_key").is_none());
    let openai = Client::new("k", "https://api.openai.com/v1")?.request_body(&req, false)?;
    assert_eq!(openai["prompt_cache_key"], json!(req.prefix_hash()));
    Ok(())
}

#[test]
fn effort_lands_in_the_body_and_the_body_table_still_wins() -> Result<()> {
    let client = Client::new("k", "http://h/v1/")?;
    let mut req = request(vec![Message::user("hi")]);
    assert!(
        client
            .request_body(&req, true)?
            .get("reasoning_effort")
            .is_none()
    );
    req.effort = Some(Effort::Low);
    assert_eq!(client.request_body(&req, true)?["reasoning_effort"], "low");
    let mut pinned = Client::new("k", "http://h/v1/")?;
    pinned.body = Some(json!({"reasoning_effort": "high"}));
    assert_eq!(pinned.request_body(&req, true)?["reasoning_effort"], "high");
    Ok(())
}

#[test]
fn effort_parses_the_four_levels_and_price_prices_fresh_cached_and_output() {
    assert_eq!(Effort::parse(" High "), Some(Effort::High));
    assert_eq!(Effort::parse("off"), Some(Effort::None));
    assert_eq!(Effort::parse("turbo"), None);
    let price = Price {
        input: 1.0,
        output: 4.0,
        cached: 0.25,
    };
    let cost = price.cost(1_000_000, 500_000, 400_000);
    assert!((cost - (0.6 + 0.1 + 2.0)).abs() < 1e-9, "{cost}");
    assert_eq!(dollars(cost), "$2.70");
    assert_eq!(dollars(0.00123), "$0.0012");
    assert!((price.cost(10, 0, 50) - 0.25 * 10.0 / 1_000_000.0).abs() < 1e-12);
}

#[test]
fn effort_none_disables_reasoning_and_off_parses_in_config() -> Result<()> {
    let client = Client::new("k", "http://h/v1/")?;
    let mut req = request(vec![Message::user("hi")]);
    req.effort = Some(Effort::None);
    assert_eq!(client.request_body(&req, true)?["reasoning_effort"], "none");
    let parsed: Effort = serde_json::from_str("\"off\"")?;
    assert_eq!(parsed, Effort::None);
    drop(serde_json::from_str::<Effort>("\"turbo\"").unwrap_err());
    Ok(())
}

#[test]
fn child_client_drops_parent_stream_callbacks() -> Result<()> {
    let parent = Client::new("k", "http://h/v1/")?
        .on_delta(|_| {})
        .on_reasoning(|_| {})
        .on_tool(|_| {});
    let child = parent
        .child()
        .ok_or_else(|| Error::Config("client does not support children".to_string()))?;

    assert!(parent.on_delta.is_some());
    assert!(parent.on_reasoning.is_some());
    assert!(parent.on_tool.is_some());
    assert!(child.on_delta.is_none());
    assert!(child.on_reasoning.is_none());
    assert!(child.on_tool.is_none());
    Ok(())
}
