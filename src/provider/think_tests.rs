use super::*;

#[test]
fn think_tags_split_from_the_answer() {
    let (text, thought) = split("<think>secret</think>ok");
    assert_eq!(text, "ok");
    assert_eq!(thought, "secret");
}

#[test]
fn unclosed_think_is_all_reasoning() {
    let (text, thought) = split("hi<think>halfway");
    assert_eq!(text, "hi");
    assert_eq!(thought, "halfway");
}

#[test]
fn frontier_text_is_unchanged() {
    let (text, thought) = split("just the answer");
    assert_eq!(text, "just the answer");
    assert!(thought.is_empty());
}

#[test]
fn object_reasoning_reads_content() {
    let value = serde_json::json!({"content": "plan"});
    assert_eq!(from_json(value).as_deref(), Some("plan"));
    assert!(from_json(serde_json::json!(null)).is_none());
    assert_eq!(
        from_json(serde_json::json!("shown")).as_deref(),
        Some("shown")
    );
}
