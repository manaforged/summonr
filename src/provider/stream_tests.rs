use super::*;

fn request() -> CompletionRequest {
    CompletionRequest {
        model: "m".to_string(),
        messages: vec![crate::provider::Message::user("hi")],
        tools: Vec::new(),
        max_output_tokens: None,
        emit_deltas: true,
        effort: None,
    }
}

fn local_stream(
    body: &'static str,
) -> Result<(Client, std::thread::JoinHandle<std::io::Result<()>>)> {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0")?;
    let address = listener.local_addr()?;
    let server = std::thread::spawn(move || -> std::io::Result<()> {
        let (mut stream, _) = listener.accept()?;
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request)?;
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    });
    Ok((
        Client::new("k", format!("http://{address}/v1"))?.on_delta(|_| {}),
        server,
    ))
}

#[test]
fn split_utf8_is_preserved_across_every_byte_boundary() -> Result<()> {
    let client = Client::new("k", "http://h/v1")?;
    for text in ["漢", "🧙"] {
        let frame = format!("data: {{\"choices\":[{{\"delta\":{{\"content\":\"{text}\"}}}}]}}\n\n");
        for split in 1..frame.len() {
            let mut stream = Stream::default();
            stream.push(&client, &frame.as_bytes()[..split])?;
            stream.push(&client, &frame.as_bytes()[split..])?;
            assert_eq!(stream.text, text, "split at byte {split}");
            assert!(!stream.text.contains('\u{fffd}'));
        }
    }
    Ok(())
}

#[test]
fn invalid_utf8_is_a_provider_error() -> Result<()> {
    let client = Client::new("k", "http://h/v1")?;
    let mut stream = Stream::default();
    assert!(stream.push(&client, b"data: \xff\n\n").is_err());
    Ok(())
}

#[test]
fn length_limited_reasoning_is_a_valid_stream_result() -> Result<()> {
    let client = Client::new("k", "http://h/v1")?;
    let mut stream = Stream::default();
    stream.push(
        &client,
        b"data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"think\"},\"finish_reason\":\"length\"}]}\n\n",
    )?;
    let message = finish(stream.message())?;
    assert_eq!(message.stop_reason, StopReason::Length);
    assert_eq!(message.reasoning_content.as_deref(), Some("think"));
    Ok(())
}

#[test]
fn think_tags_in_content_stream_as_reasoning() -> Result<()> {
    let client = Client::new("k", "http://h/v1")?;
    let mut stream = Stream::default();
    stream.push(
        &client,
        b"data: {\"choices\":[{\"delta\":{\"content\":\"<thi\"}}]}\n\n",
    )?;
    stream.push(
        &client,
        b"data: {\"choices\":[{\"delta\":{\"content\":\"nk>secret</think>ok\"}}]}\n\n",
    )?;
    stream.flush(&client);
    let message = finish(stream.message())?;
    assert_eq!(message.content.as_deref(), Some("ok"));
    assert_eq!(message.reasoning_content.as_deref(), Some("secret"));
    Ok(())
}

#[test]
fn a_streaming_tool_call_announces_its_name_once_before_the_round_ends() -> Result<()> {
    use std::sync::{Arc, Mutex};
    let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = calls.clone();
    let client = Client::new("k", "http://h/v1")?.on_tool(move |name| {
        if let Ok(mut names) = sink.lock() {
            names.push(name.to_string());
        }
    });
    let mut stream = Stream::default();
    stream.push(
        &client,
        b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"write\"}}]}}]}\n\n",
    )?;
    assert_eq!(
        *calls.lock().unwrap_or_else(|e| e.into_inner()),
        vec!["write"]
    );
    stream.push(
        &client,
        b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"path\\\":\\\"a\\\"}\"}}]}}]}\n\n",
    )?;
    assert_eq!(
        *calls.lock().unwrap_or_else(|e| e.into_inner()),
        vec!["write"]
    );
    let message = finish(stream.message())?;
    assert_eq!(message.tool_calls.len(), 1);
    assert_eq!(message.tool_calls[0].function.name, "write");
    Ok(())
}

#[test]
fn a_wild_tool_call_index_is_ignored_not_allocated() -> Result<()> {
    let client = Client::new("k", "http://h/v1")?;
    let mut stream = Stream::default();
    stream.push(
        &client,
        b"data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":100000000,\"id\":\"x\",\"function\":{\"name\":\"f\"}}]}}]}\n\n",
    )?;
    assert!(stream.calls.is_empty());
    Ok(())
}

#[test]
fn contiguous_tool_calls_have_no_fixed_limit() {
    let mut calls = Vec::new();
    for index in 0..100 {
        merge_call(
            &mut calls,
            CallDelta {
                index: Some(index),
                id: Some(format!("call-{index}")),
                function: Some(FunctionDelta {
                    name: Some("read_file".to_string()),
                    arguments: Some("{}".to_string()),
                }),
            },
        );
    }
    assert_eq!(calls.len(), 100);
    assert_eq!(calls[99].id, "call-99");
}

#[test]
fn a_mid_stream_error_is_a_provider_error() -> Result<()> {
    let client = Client::new("k", "http://h/v1")?;
    let mut stream = Stream::default();
    stream.push(
        &client,
        b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
    )?;
    let result = stream.push(
        &client,
        b"data: {\"error\":{\"message\":\"overloaded\",\"code\":524}}\n\n",
    );
    let error = result.expect_err("mid-stream error must surface, not be dropped");
    assert!(error.to_string().contains("overloaded"), "got: {error}");
    assert!(
        error
            .provider_failure()
            .is_some_and(|failure| failure.partial_output)
    );
    Ok(())
}

#[test]
fn malformed_frame_after_output_preserves_partial_state() -> Result<()> {
    let client = Client::new("k", "http://h/v1")?;
    let mut stream = Stream::default();
    stream.push(
        &client,
        b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
    )?;
    let error = stream
        .push(&client, b"data: {not-json}\n\n")
        .expect_err("malformed frame must surface");
    assert!(
        error
            .provider_failure()
            .is_some_and(|failure| failure.partial_output)
    );
    Ok(())
}

#[test]
fn clean_eof_before_a_terminal_frame_is_rejected_with_partial_state() -> Result<()> {
    let client = Client::new("k", "http://h/v1")?;
    let mut stream = Stream::default();
    stream.push(
        &client,
        b"data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
    )?;
    let error = stream
        .complete(&client)
        .expect_err("clean EOF must not complete a partial stream");
    assert!(error.to_string().contains("finish reason or [DONE]"));
    assert!(
        error
            .provider_failure()
            .is_some_and(|failure| failure.partial_output)
    );
    Ok(())
}

#[test]
fn finish_reason_or_done_are_supported_terminal_frames() -> Result<()> {
    let client = Client::new("k", "http://h/v1")?;
    for terminal in [
        b"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\n"
            .as_slice(),
        b"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: [DONE]\n\n".as_slice(),
    ] {
        let mut stream = Stream::default();
        stream.push(&client, terminal)?;
        let message = stream.complete(&client)?;
        assert_eq!(message.content.as_deref(), Some("ok"));
        assert_eq!(message.stop_reason, StopReason::Stop);
    }
    Ok(())
}

#[tokio::test]
async fn client_rejects_a_tool_stream_that_closes_before_terminal_frame() -> Result<()> {
    let body = "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",\"function\":{\"name\":\"write\",\"arguments\":\"{}\"}}]}}]}\n\n";
    let (client, server) = local_stream(body)?;
    let error = client
        .complete_stream(&request())
        .await
        .expect_err("incomplete tool stream must fail before handoff");
    assert!(
        error
            .provider_failure()
            .is_some_and(|failure| failure.partial_output)
    );
    server
        .join()
        .map_err(|_panic| Error::Config("stream fixture panicked".to_string()))??;
    Ok(())
}

#[tokio::test]
async fn client_accepts_a_complete_stream_with_terminal_frames() -> Result<()> {
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\ndata: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
    let (client, server) = local_stream(body)?;
    let message = client.complete_stream(&request()).await?;
    assert_eq!(message.content.as_deref(), Some("ok"));
    assert_eq!(message.stop_reason, StopReason::Stop);
    server
        .join()
        .map_err(|_panic| Error::Config("stream fixture panicked".to_string()))??;
    Ok(())
}

#[tokio::test]
async fn client_rejects_truncated_data_after_a_terminal_frame() -> Result<()> {
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\ndata: {\"choices\"";
    let (client, server) = local_stream(body)?;
    let error = client
        .complete_stream(&request())
        .await
        .expect_err("trailing incomplete data must fail");
    assert!(error.to_string().contains("incomplete SSE frame"));
    assert!(
        error
            .provider_failure()
            .is_some_and(|failure| failure.partial_output)
    );
    server
        .join()
        .map_err(|_panic| Error::Config("stream fixture panicked".to_string()))??;
    Ok(())
}

#[tokio::test]
async fn client_preserves_partial_state_for_an_in_band_error() -> Result<()> {
    let body = "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\ndata: {\"error\":{\"message\":\"overloaded\"}}\n\n";
    let (client, server) = local_stream(body)?;
    let error = client
        .complete_stream(&request())
        .await
        .expect_err("in-band provider errors must fail the client request");
    assert!(error.to_string().contains("overloaded"));
    assert!(
        error
            .provider_failure()
            .is_some_and(|failure| failure.partial_output)
    );
    server
        .join()
        .map_err(|_panic| Error::Config("stream fixture panicked".to_string()))??;
    Ok(())
}
