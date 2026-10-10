use super::*;

fn call(id: &str, output: &str) -> [Event; 2] {
    [
        Event::ToolCall {
            id: id.to_string(),
            name: "run_command".to_string(),
            arguments: "{}".to_string(),
        },
        Event::ToolResult {
            id: id.to_string(),
            success: false,
            mutated: false,
            output: output.to_string(),
            millis: None,
            patch: None,
        },
    ]
}

#[test]
fn receipt_fits_one_summary_line_and_caps_lists() {
    let events: Vec<Event> = (0..8)
        .flat_map(|index| call(&index.to_string(), &format!("exit {}", index % 7)))
        .collect();
    let changes = (0..30).map(|index| format!("src/file{index}.rs")).collect();
    let receipt = Receipt::new(changes, Vec::new(), "abc (HEAD)".to_string(), &events);
    let mut lines = receipt.text.lines();
    assert_eq!(
        lines.next(),
        Some("Turn receipt · 0 read · 0 searches · 8 commands · 8 failed · 30 changed · no check")
    );
    let failed = receipt
        .text
        .lines()
        .find(|line| line.starts_with("- Failed:"))
        .unwrap();
    assert_eq!(
        failed,
        "- Failed: run_command: exit 0; run_command: exit 1; run_command: exit 2; run_command: exit 3; run_command: exit 4; +2 more"
    );
    let changed = receipt
        .text
        .lines()
        .find(|line| line.starts_with("- Changed:"))
        .unwrap();
    assert!(changed.ends_with("`src/file4.rs`, +25 more"), "{changed}");
    assert_eq!(receipt.text.lines().count(), 5, "{}", receipt.text);
}
