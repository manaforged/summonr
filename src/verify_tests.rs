use super::*;

#[test]
fn a_turn_receipt_records_the_check_and_does_not_narrate_the_turn() {
    let events = vec![
        Event::ToolCall {
            id: "c1".to_string(),
            name: "run_command".to_string(),
            arguments: r#"{"command":"loop verify"}"#.to_string(),
        },
        Event::ToolResult {
            id: "c1".to_string(),
            success: true,
            mutated: false,
            output: "PASS".to_string(),
            millis: None,
            patch: None,
        },
    ];
    let receipt = from_turn(
        vec!["src/runtime/turn.rs".to_string()],
        "abc (HEAD)".to_string(),
        &events,
        &["loop verify".to_string()],
    );
    assert!(receipt.text.is_empty(), "{}", receipt.text);
    assert!(receipt.verified);
    assert_eq!(receipt.changes, vec!["src/runtime/turn.rs".to_string()]);
    assert!(
        receipt
            .checks
            .iter()
            .any(|check| check.command.contains("loop verify") && check.passed),
        "{receipt:?}"
    );
}
