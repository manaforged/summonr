use super::*;

#[test]
fn framing_reads_each_dialect_off_the_wire() {
    let anthropic = b"event: message_start\ndata: {\"type\":\"message_start\"}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\"}\n\n";
    let mut framing = Frames::default();
    let frames = framing
        .push(anthropic, anthropic::Wire::sentinel())
        .unwrap();
    assert_eq!(
        frames.len(),
        2,
        "the event: lines are not frames of their own"
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(frames[0]).unwrap()["type"],
        "message_start"
    );

    let gemini = b"data: {\"candidates\":[]}\r\n\r\n";
    let mut framing = Frames::default();
    let frames = framing.push(gemini, gemini::Wire::sentinel()).unwrap();
    assert_eq!(frames.len(), 1, "a CRLF boundary must frame like an LF one");
}

#[test]
fn a_frame_split_across_chunks_is_not_lost() {
    let mut frames = Frames::default();
    assert!(
        frames
            .push(b"data: {\"type\":\"mes", None)
            .unwrap()
            .is_empty(),
        "half a frame yields nothing yet"
    );
    let rest = frames.push(b"sage_start\"}\n\n", None).unwrap();
    assert_eq!(rest.len(), 1, "the halves must join");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(rest[0]).unwrap()["type"],
        "message_start"
    );
}

#[test]
fn a_sentinel_ends_the_stream_only_where_a_wire_declares_one() {
    let done = b"data: {\"a\":1}\n\ndata: [DONE]\n\ndata: {\"b\":2}\n\n";
    let mut declared = Frames::default();
    let frames = declared.push(done, Some("[DONE]")).unwrap();
    assert_eq!(frames.len(), 1, "nothing after the sentinel is read");
    assert!(declared.ended());

    let mut undeclared = Frames::default();
    let frames = undeclared.push(done, None).unwrap();
    assert_eq!(
        frames.len(),
        3,
        "a wire with no sentinel reads the marker as a payload its parse rejects"
    );
    assert_eq!(frames[1], "[DONE]");
    assert!(!undeclared.ended());
}

#[test]
fn a_malformed_frame_is_an_error_at_parse() {
    let mixed = b"data: not json\n\ndata: {\"good\":true}\n\n";
    let mut framing = Frames::default();
    let frames = framing.push(mixed, None).unwrap();
    assert_eq!(frames.len(), 2, "framing yields each data payload");
    assert!(
        serde_json::from_str::<serde_json::Value>(frames[0]).is_err(),
        "the bad payload fails typed parse instead of being dropped"
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(frames[1]).unwrap()["good"],
        true
    );
}

#[test]
fn a_comment_or_a_blank_keepalive_is_ignored() {
    let noise = b": keepalive\n\ndata:\n\ndata: {\"real\":1}\n\n";
    let mut framing = Frames::default();
    let frames = framing.push(noise, None).unwrap();
    assert_eq!(frames.len(), 1, "got {frames:?}");
}

#[test]
fn bytes_reach_an_answer_through_the_contract_alone() {
    fn run<W: Chat>(bytes: &[u8], model: &str) -> AssistantMessage {
        let mut frames = Frames::default();
        let mut stream = W::Stream::new(model);
        for frame in frames.push(bytes, W::sentinel()).unwrap() {
            drop(stream.on(frame).unwrap());
        }
        stream.finish()
    }
    let anthropic = b"event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\"}}\n\nevent: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"text\":\"hi\"}}\n\n";
    assert_eq!(
        run::<anthropic::Wire>(anthropic, "claude-opus-5")
            .content
            .as_deref(),
        Some("hi")
    );
    let gemini = b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"hi\"}]}}]}\n\n";
    assert_eq!(
        run::<gemini::Wire>(gemini, "gemini-2.5-flash")
            .content
            .as_deref(),
        Some("hi"),
        "raw bytes to an answer must need nothing the trait does not provide"
    );
}

#[test]
fn a_provider_that_never_frames_is_cut_off_rather_than_filling_memory() {
    let mut frames = Frames::default();
    let chunk = vec![b'x'; 1024 * 1024];
    let mut error = None;
    for _ in 0..(MOST_PENDING / chunk.len() + 1) {
        if let Err(cut) = frames.push(&chunk, None) {
            error = Some(cut);
            break;
        }
    }
    let error = error.expect("a stream with no boundary must stop, not grow without end");
    assert!(
        error.to_string().contains("no frame boundary"),
        "got {error}"
    );
}

#[test]
fn bad_bytes_are_reported_rather_than_losing_the_turn() {
    let mut frames = Frames::default();
    let mut bytes = b"data: ".to_vec();
    bytes.extend_from_slice(&[0xff, 0xfe]);
    bytes.extend_from_slice(b"\n\n");
    let error = frames
        .push(&bytes, None)
        .expect_err("output we cannot read must be reported, not silently dropped");
    assert!(
        error.to_string().contains("invalid SSE bytes"),
        "got {error}"
    );
}
