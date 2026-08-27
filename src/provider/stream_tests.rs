use super::*;

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
    // Name arrives first; arguments stream in later frames.
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
    // Fires exactly once — later argument frames do not re-announce.
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
    assert!(stream.calls.len() <= MAX_TOOL_CALLS);
    Ok(())
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
    Ok(())
}
