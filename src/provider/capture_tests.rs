use super::*;

#[test]
fn capture_is_off_unless_a_directory_is_named() {
    assert!(pick(None).is_none(), "capture must never run by default");
    assert!(pick(Some(String::new())).is_none());
    assert!(pick(Some("   ".to_string())).is_none());
}

#[test]
fn every_wire_has_a_name_a_file_can_carry() {
    for wire in [
        Wire::Anthropic,
        Wire::Gemini,
        Wire::Responses,
        Wire::Completions,
        Wire::Decisions,
    ] {
        assert!(!wire_name(wire).is_empty());
    }
    assert_eq!(wire_name(Wire::Anthropic), "anthropic");
    assert_eq!(wire_name(Wire::Gemini), "gemini");
}

#[test]
fn a_sink_writes_one_json_line_per_frame() {
    let dir = std::env::temp_dir().join(format!("summonr-capture-{}", std::process::id()));
    let sink =
        Sink::open_in(dir.clone(), Wire::Gemini, "stream").expect("a directory opens a sink");
    sink.write("frame", r#"{"candidates":[]}"#);
    sink.write("frame", r#"{"usageMetadata":{}}"#);
    let text = std::fs::read_to_string(sink.path()).unwrap();
    assert_eq!(text.lines().count(), 2, "one line per frame: {text}");
    let first: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(first["at"], "frame");
    assert!(first["body"].as_str().unwrap().contains("candidates"));
    assert!(
        sink.path().to_string_lossy().contains("gemini"),
        "the wire must be readable from the filename"
    );
    drop(std::fs::remove_dir_all(&dir));
}

#[test]
fn two_sinks_in_one_run_do_not_share_a_file() {
    let dir = std::env::temp_dir().join(format!("summonr-capture-two-{}", std::process::id()));
    let one = Sink::open_in(dir.clone(), Wire::Anthropic, "stream").unwrap();
    let two = Sink::open_in(dir.clone(), Wire::Anthropic, "stream").unwrap();
    assert_ne!(
        one.path(),
        two.path(),
        "a second call in the same second must not append to the first capture"
    );
    drop(std::fs::remove_dir_all(&dir));
}
