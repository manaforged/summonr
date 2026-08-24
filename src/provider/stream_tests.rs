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
