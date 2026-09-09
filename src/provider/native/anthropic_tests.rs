use super::*;

#[test]
fn oversized_continuation_is_rejected_before_parsing() -> Result<()> {
    let data = " ".repeat(MAX_BYTES / 2);
    let frame = format!(r#"{{"type":"ping"}}{data}"#);
    let mut capture = Capture::default();
    capture.push(RawFrameRef {
        index: 0,
        event: Some("ping"),
        data: &frame,
        elapsed_us: 0,
    })?;
    let error = capture
        .push(RawFrameRef {
            index: 1,
            event: Some("ping"),
            data: &frame,
            elapsed_us: 0,
        })
        .expect_err("combined stream data must respect the continuation limit");
    assert!(error.to_string().contains("exceeds 16 MiB"));
    let body = serde_json::json!({"content":[{"type":"text","text":"x".repeat(MAX_BYTES)}]});
    assert!(
        blocks(Some(&body))
            .expect_err("nonstreaming content must be bounded")
            .to_string()
            .contains("exceeds 16 MiB")
    );
    Ok(())
}
