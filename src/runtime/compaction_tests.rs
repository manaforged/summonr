use super::*;
use crate::provider::Image;

#[test]
fn context_estimate_counts_reasoning_sent_back_to_provider() {
    let message = Message {
        role: Role::Assistant,
        content: "answer".to_string(),
        images: Vec::new(),
        tool_calls: Vec::new(),
        tool_call_id: None,
        reasoning: Some("private reasoning".to_string()),
    };

    assert_eq!(message_chars(&message), "answerprivate reasoning".len());
}

#[test]
fn context_estimate_does_not_count_image_bytes_as_text() {
    let message = Message::user(UserInput {
        text: "look".to_string(),
        images: vec![Image {
            media_type: "image/png".to_string(),
            bytes: b"encoded-image".to_vec(),
        }],
    });

    assert_eq!(message_chars(&message), "look".len());
}

#[test]
fn estimate_uses_observed_tokens_plus_new_text() {
    assert_eq!(estimate(1_400, Some((1_000, 500))), 600);
    assert_eq!(estimate(400, None), 100);
}

#[test]
fn context_pressure_starts_at_80_percent() {
    assert!(!under_pressure(799, 1_000));
    assert!(under_pressure(800, 1_000));
}

#[test]
fn overflow_recovery_preserves_the_oldest_tool_prefix() {
    let mut messages = vec![
        Message::system("system"),
        Message::tool("one", "a".repeat(20)),
        Message::tool("two", "b".repeat(20)),
        Message::tool("three", "c".repeat(20)),
        Message::tool("four", "d".repeat(20)),
    ];

    assert!(force_compact(&mut messages));
    assert_eq!(messages[1].content, "a".repeat(20));
    assert_eq!(messages[2].content, "b".repeat(20));
    assert!(messages[3].content.starts_with("[cleared"));
    assert!(messages[4].content.starts_with("[cleared"));
}
