use super::*;

#[test]
fn capture_is_off_unless_a_directory_is_named() {
    assert!(
        pick(None).expect("capture selection").is_none(),
        "capture must never run by default"
    );
    assert!(
        pick(Some(String::new()))
            .expect("capture selection")
            .is_none()
    );
    assert!(
        pick(Some("   ".to_string()))
            .expect("capture selection")
            .is_none()
    );
}

#[test]
fn a_sink_writes_one_json_line_per_frame() {
    let dir = std::env::temp_dir().join(format!("summonr-capture-{}", std::process::id()));
    let sink = Sink::open_in(dir.clone(), Wire::Gemini, "stream", "session-a")
        .expect("a directory opens a sink");
    sink.write(Stage::Response, br#"{"candidates":[]}"#)
        .expect("capture frame");
    sink.write(Stage::Response, br#"{"usageMetadata":{}}"#)
        .expect("capture frame");
    sink.write(Stage::Chunk, &[0xf0, 0x9f])
        .expect("capture split UTF-8 bytes");
    let text = std::fs::read_to_string(sink.path()).unwrap();
    assert_eq!(text.lines().count(), 3, "one line per frame: {text}");
    let first: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(first["session"], "session-a");
    assert_eq!(first["record"]["kind"], "response");
    assert_eq!(first["record"]["body"], "eyJjYW5kaWRhdGVzIjpbXX0=");
    let last: serde_json::Value =
        serde_json::from_str(text.lines().last().expect("third frame")).expect("capture JSON");
    assert_eq!(last["record"]["body"], "8J8=");
    assert_eq!(last["sequence"], 3);
    assert!(
        sink.path().to_string_lossy().contains("gemini"),
        "the wire must be readable from the filename"
    );
    std::fs::remove_file(sink.path()).expect("remove the test capture file");
    let failure = sink
        .write(Stage::Response, b"lost frame")
        .expect_err("capture file was removed");
    assert!(
        crate::runtime::billable(&failure),
        "received provider output must retain its uncertain charge"
    );
    let unsent = sink
        .write(Stage::Request, b"unsent request")
        .expect_err("capture file was removed");
    assert!(
        !crate::runtime::billable(&unsent),
        "an unsent request is not billable"
    );
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn two_sinks_in_one_run_do_not_share_a_file() {
    let dir = std::env::temp_dir().join(format!("summonr-capture-two-{}", std::process::id()));
    let one =
        Sink::open_in(dir.clone(), Wire::Anthropic, "stream", "session-a").expect("capture sink");
    let two =
        Sink::open_in(dir.clone(), Wire::Anthropic, "stream", "session-a").expect("capture sink");
    assert_ne!(
        one.path(),
        two.path(),
        "a second call in the same second must not append to the first capture"
    );
    drop(std::fs::remove_dir_all(&dir));
}
